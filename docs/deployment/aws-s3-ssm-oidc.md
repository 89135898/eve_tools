# AWS S3 + SSM + OIDC 部署指南

本文说明如何把 EveTools 后端二进制文件从 GitHub Actions 部署到一台固定 EC2 实例。部署链路是 GitHub Actions 构建、S3 保存 release artifact、SSM Run Command 在 EC2 上激活 release。

此流程不需要 SSH，也不需要在 GitHub 保存长期 AWS access key。

## 部署内容

GitHub Actions 会构建并部署两个 Linux x86_64 二进制文件：

- `evetools-http-api`
- `sync-public-market-region`

发布包结构：

```text
bin/evetools-http-api
bin/sync-public-market-region
```

S3 artifact 路径：

```text
s3://$DEPLOY_BUCKET/releases/$GITHUB_SHA/evetools-$GITHUB_SHA-linux-x86_64.tar.gz
```

workflow 使用 S3 conditional write 上传 artifact；同一个 commit SHA 的 artifact 已存在时不会覆盖对象。

EC2 release 路径：

```text
/opt/evetools/releases/$GITHUB_SHA
/opt/evetools/current
```

## GitHub 配置

在 GitHub 仓库的 Variables 或 Secrets 中配置：

| 名称 | 说明 |
| --- | --- |
| `AWS_REGION` | EC2、S3、SSM 所在 AWS region |
| `AWS_ROLE_ARN` | GitHub Actions 通过 OIDC assume 的 IAM role ARN |
| `DEPLOY_BUCKET` | 保存 release artifact 的 S3 bucket 名称 |
| `EC2_INSTANCE_ID` | 固定目标 EC2 instance id |

不要配置 AWS access key。workflow 使用 GitHub OIDC 获取临时 AWS 凭证。

## AWS OIDC Provider

如果 AWS 账号还没有 GitHub OIDC provider，创建 provider：

```bash
aws iam create-open-id-connect-provider \
  --url https://token.actions.githubusercontent.com \
  --client-id-list sts.amazonaws.com \
  --thumbprint-list 6938fd4d98bab03faadb97b34396831e3780aea1
```

如果 provider 已存在，不需要重复创建。

## GitHub Actions 部署 Role

部署 role 的 trust policy 必须限制到当前仓库。下面的 JSON 使用 `owner/repo` 和 `main` 作为示例；如果仓库当前生产分支是 `master`，把 `repo:owner/repo:ref:refs/heads/main` 改成 `repo:owner/repo:ref:refs/heads/master`，或者为两个分支各加一条允许条件。

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Principal": {
        "Federated": "arn:aws:iam::111122223333:oidc-provider/token.actions.githubusercontent.com"
      },
      "Action": "sts:AssumeRoleWithWebIdentity",
      "Condition": {
        "StringEquals": {
          "token.actions.githubusercontent.com:aud": "sts.amazonaws.com",
          "token.actions.githubusercontent.com:sub": "repo:owner/repo:ref:refs/heads/main"
        }
      }
    }
  ]
}
```

部署 role policy：

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": "s3:PutObject",
      "Resource": "arn:aws:s3:::evetools-deploy/releases/*"
    },
    {
      "Effect": "Allow",
      "Action": "ssm:SendCommand",
      "Resource": [
        "arn:aws:ec2:ap-northeast-1:111122223333:instance/i-0123456789abcdef0",
        "arn:aws:ssm:ap-northeast-1::document/AWS-RunShellScript"
      ]
    },
    {
      "Effect": "Allow",
      "Action": "ssm:GetCommandInvocation",
      "Resource": "*"
    }
  ]
}
```

把示例中的账号、region、bucket、仓库名和 instance id 改成生产值后再应用。

## EC2 Instance Profile

EC2 instance profile 需要：

- `AmazonSSMManagedInstanceCore`
- 读取 release artifact 的 S3 权限

S3 读取权限示例：

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": "s3:GetObject",
      "Resource": "arn:aws:s3:::evetools-deploy/releases/*"
    }
  ]
}
```

## EC2 运行环境

目标 EC2 必须已经出现在 Systems Manager managed instances 中。

实例需要安装：

- AWS CLI
- `tar`
- `systemd`

目标实例需要是 x86_64 Linux，并且运行时兼容 GitHub `ubuntu-24.04` runner 构建出的 GNU/Linux 二进制。

创建目录：

```bash
sudo mkdir -p /opt/evetools/releases
sudo chown root:root /opt/evetools /opt/evetools/releases
```

服务端环境变量放在 EC2 本地：

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

不要把 `EVETOOLS_DATABASE_URL` 放进 GitHub Actions、release artifact 或桌面端配置。

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

如果要让 EC2 定时同步公开市场数据，可以创建 timer。workflow 会执行 `systemctl try-restart evetools-market-sync.timer || true`，所以没有 timer 时部署不会失败。

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

## 手动验证

在 EC2 上验证服务：

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
sudo ln -sfn /opt/evetools/releases/OLD_GIT_SHA /opt/evetools/current
sudo systemctl restart evetools-http-api.service
```

`OLD_GIT_SHA` 必须替换为 `/opt/evetools/releases` 中真实存在的目录名。
