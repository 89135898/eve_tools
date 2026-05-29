# SSH 部署指南

本文说明如何通过 GitHub Actions + SSH 把 EveTools 后端二进制文件部署到一台固定服务器。

这个方案比 S3 + SSM + OIDC 简单：GitHub Actions 构建 release artifact，用 `scp` 上传到服务器，再用 `ssh` 执行解包、切换 symlink 和重启 systemd 服务。

## 部署内容

GitHub Actions 会构建并部署两个 Linux x86_64 二进制文件：

- `evetools-http-api`
- `sync-public-market-region`

发布包结构：

```text
bin/evetools-http-api
bin/sync-public-market-region
```

服务器 release 路径：

```text
/opt/evetools/releases/$GITHUB_SHA
/opt/evetools/current
```

## GitHub Secrets

在 GitHub 仓库的 Secrets 中配置：

| 名称 | 说明 |
| --- | --- |
| `SSH_HOST` | 服务器公网 IP 或域名 |
| `SSH_USER` | SSH 登录用户 |
| `SSH_PRIVATE_KEY` | GitHub Actions 使用的私钥内容 |
| `SSH_KNOWN_HOSTS` | 服务器 host key，通常来自 `ssh-keyscan -H <host>` |

不要把 `EVETOOLS_DATABASE_URL` 放进 GitHub。数据库连接串只放在服务器本地。

## SSH Key

在本地生成专用部署 key：

```bash
ssh-keygen -t ed25519 -C "evetools-github-actions-deploy" -f evetools_deploy_key
```

把公钥加入服务器部署用户的 `~/.ssh/authorized_keys`：

```bash
cat evetools_deploy_key.pub
```

把私钥文件 `evetools_deploy_key` 的完整内容放入 GitHub Secret：

```text
SSH_PRIVATE_KEY
```

把服务器 host key 放入 GitHub Secret：

```bash
ssh-keyscan -H <server-host>
```

对应 GitHub Secret：

```text
SSH_KNOWN_HOSTS
```

## 服务器要求

服务器需要是 x86_64 Linux，并且运行时兼容 GitHub `ubuntu-24.04` runner 构建出的 GNU/Linux 二进制。

服务器需要安装：

- `ssh`
- `tar`
- `systemd`

目标 SSH 用户需要能免密码执行 workflow 中用到的 `sudo` 命令。最简单的早期配置是给部署用户 NOPASSWD sudo；生产环境可以再收窄到 `mkdir`、`tar`、`chown`、`chmod`、`ln`、`mv`、`systemctl`。

创建目录：

```bash
sudo mkdir -p /opt/evetools/releases
sudo chown root:root /opt/evetools /opt/evetools/releases
```

服务端环境变量放在服务器本地：

```bash
sudo mkdir -p /etc/evetools
sudo touch /etc/evetools/evetools.env
sudo chown root:root /etc/evetools/evetools.env
sudo chmod 600 /etc/evetools/evetools.env
```

`/etc/evetools/evetools.env` 示例：

```bash
EVETOOLS_DATABASE_URL=postgresql://user:password@host:5432/postgres?sslmode=require
EVETOOLS_HTTP_ADDR=0.0.0.0:8080
```

## systemd 服务

`/etc/systemd/system/evetools-http-api.service`：

```ini
[Unit]
Description=EveTools HTTP API
After=network-online.target
Wants=network-online.target

[Service]
EnvironmentFile=/etc/evetools/evetools.env
ExecStart=/opt/evetools/current/bin/evetools-http-api
Restart=always
RestartSec=5
User=root
Group=root

[Install]
WantedBy=multi-user.target
```

启用服务：

```bash
sudo systemctl daemon-reload
sudo systemctl enable evetools-http-api.service
```

第一次部署前服务可能因为 `/opt/evetools/current` 不存在而无法启动，这是正常状态。首次 GitHub Actions 部署成功后会创建 symlink 并重启服务。

## 可选同步 Timer

如果要让服务器定时同步公开市场数据，可以创建 timer。workflow 会执行 `systemctl try-restart evetools-market-sync.timer || true`，所以没有 timer 时部署不会失败。

`/etc/systemd/system/evetools-market-sync.service`：

```ini
[Unit]
Description=EveTools public market sync
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
EnvironmentFile=/etc/evetools/evetools.env
ExecStart=/opt/evetools/current/bin/sync-public-market-region --all-default-regions --started-by production-systemd-timer --lease-ttl-seconds 1200 --max-age-seconds 600 --json
User=root
Group=root
```

`/etc/systemd/system/evetools-market-sync.timer`：

```ini
[Unit]
Description=Run EveTools public market sync every 10 minutes

[Timer]
OnBootSec=2min
OnUnitActiveSec=10min
Unit=evetools-market-sync.service

[Install]
WantedBy=timers.target
```

启用 timer：

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now evetools-market-sync.timer
```

## 触发部署

workflow 文件：

```text
.github/workflows/deploy-prod.yml
```

触发方式：

- push 到 `main`
- push 到 `master`
- GitHub Actions 手动 `workflow_dispatch`

部署时 Actions 会：

1. 运行后端测试。
2. 构建 release 二进制。
3. 打包 `evetools-$GITHUB_SHA-linux-x86_64.tar.gz`。
4. 用 `scp` 上传到服务器 `/tmp`。
5. 用 `ssh` 解包到 `/opt/evetools/releases/$GITHUB_SHA`。
6. 原子切换 `/opt/evetools/current`。
7. 重启 `evetools-http-api.service`。
8. 如果存在，重启 `evetools-market-sync.timer`。

## 手动验证

在服务器上验证服务：

```bash
systemctl status evetools-http-api.service
curl http://127.0.0.1:8080/health
curl http://127.0.0.1:8080/ready
```

查看部署后的 symlink：

```bash
readlink /opt/evetools/current
ls -la /opt/evetools/current/bin
```

查看同步 timer：

```bash
systemctl list-timers evetools-market-sync.timer
journalctl -u evetools-market-sync.service -n 100 --no-pager
```

## 手动回滚

列出已有 release：

```bash
ls -1 /opt/evetools/releases
```

切回旧 release：

```bash
sudo ln -sfn /opt/evetools/releases/OLD_GIT_SHA /opt/evetools/.current-rollback
sudo mv -Tf /opt/evetools/.current-rollback /opt/evetools/current
sudo systemctl restart evetools-http-api.service
```

`OLD_GIT_SHA` 必须替换为 `/opt/evetools/releases` 中真实存在的目录名。

