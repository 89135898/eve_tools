# S3 SSM OIDC Production Deploy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a production deployment path that builds EveTools backend binaries in GitHub Actions, uploads a release artifact to S3, and activates it on a fixed EC2 instance through SSM using GitHub OIDC.

**Architecture:** GitHub Actions performs all build, package, upload, and deployment orchestration. AWS authentication uses OIDC through `aws-actions/configure-aws-credentials`; the EC2 instance receives a single SSM `AWS-RunShellScript` command that downloads the S3 artifact, switches `/opt/evetools/current`, and restarts `evetools-http-api.service`.

**Tech Stack:** GitHub Actions, Rust 1.82.0, AWS CLI, AWS S3, AWS Systems Manager Run Command, GitHub OIDC, systemd.

---

## File Map

- Create `.github/workflows/deploy-prod.yml`: production deployment workflow for tests, release builds, S3 upload, SSM activation, and SSM status polling.
- Create `docs/deployment/aws-s3-ssm-oidc.md`: operator documentation for AWS resources, GitHub configuration, EC2 runtime contract, and manual verification.
- Modify `README.md`: add a short link to the deployment guide from the production operations section.

The current checked-out branch is `master`, while the design also requires `main`. The workflow should trigger on both `main` and `master` so it works for the current repository and still satisfies the intended future default branch.

---

### Task 1: Add Production Deploy Workflow

**Files:**
- Create: `.github/workflows/deploy-prod.yml`

- [ ] **Step 1: Write the failing workflow existence check**

Run:

```bash
test -f .github/workflows/deploy-prod.yml
```

Expected: FAIL with exit code 1 because the workflow file does not exist yet.

- [ ] **Step 2: Create the workflow directory**

Run:

```bash
mkdir -p .github/workflows
```

Expected: PASS and `.github/workflows` exists.

- [ ] **Step 3: Add the production deployment workflow**

Create `.github/workflows/deploy-prod.yml` with this exact content:

```yaml
name: Deploy production backend

on:
  push:
    branches:
      - main
      - master
  workflow_dispatch:

permissions:
  contents: read
  id-token: write

concurrency:
  group: deploy-prod
  cancel-in-progress: false

env:
  RUST_VERSION: "1.82.0"
  ARTIFACT_NAME: evetools-${{ github.sha }}-linux-x86_64.tar.gz
  AWS_REGION: ${{ vars.AWS_REGION != '' && vars.AWS_REGION || secrets.AWS_REGION }}
  AWS_ROLE_ARN: ${{ vars.AWS_ROLE_ARN != '' && vars.AWS_ROLE_ARN || secrets.AWS_ROLE_ARN }}
  DEPLOY_BUCKET: ${{ vars.DEPLOY_BUCKET != '' && vars.DEPLOY_BUCKET || secrets.DEPLOY_BUCKET }}
  EC2_INSTANCE_ID: ${{ vars.EC2_INSTANCE_ID != '' && vars.EC2_INSTANCE_ID || secrets.EC2_INSTANCE_ID }}

jobs:
  deploy:
    name: Build and deploy backend
    runs-on: ubuntu-24.04
    timeout-minutes: 45

    steps:
      - name: Checkout repository
        uses: actions/checkout@v4

      - name: Validate deployment configuration
        shell: bash
        run: |
          set -euo pipefail

          missing=0
          for name in AWS_REGION AWS_ROLE_ARN DEPLOY_BUCKET EC2_INSTANCE_ID; do
            if [ -z "${!name}" ]; then
              echo "::error::${name} is required as a GitHub repository variable or secret"
              missing=1
            fi
          done

          if [ "$missing" -ne 0 ]; then
            exit 1
          fi

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable
        with:
          toolchain: ${{ env.RUST_VERSION }}

      - name: Cache Cargo registry and build outputs
        uses: Swatinem/rust-cache@v2
        with:
          cache-on-failure: true

      - name: Run backend tests
        shell: bash
        run: |
          set -euo pipefail
          cargo test \
            -p evetools-domain \
            -p evetools-esi \
            -p evetools-db \
            -p evetools-api \
            -p evetools-http-api \
            -p evetools-worker

      - name: Build release binaries
        shell: bash
        run: |
          set -euo pipefail
          cargo build --release -p evetools-http-api --bin evetools-http-api
          cargo build --release -p evetools-worker --bin sync-public-market-region

      - name: Package release artifact
        shell: bash
        run: |
          set -euo pipefail
          rm -rf dist
          mkdir -p dist/bin
          cp target/release/evetools-http-api dist/bin/evetools-http-api
          cp target/release/sync-public-market-region dist/bin/sync-public-market-region
          tar -C dist -czf "${ARTIFACT_NAME}" bin
          tar -tzf "${ARTIFACT_NAME}"

      - name: Configure AWS credentials
        uses: aws-actions/configure-aws-credentials@v4
        with:
          aws-region: ${{ env.AWS_REGION }}
          role-to-assume: ${{ env.AWS_ROLE_ARN }}
          role-session-name: evetools-github-actions-deploy

      - name: Upload release artifact to S3
        shell: bash
        run: |
          set -euo pipefail
          aws s3 cp \
            "${ARTIFACT_NAME}" \
            "s3://${DEPLOY_BUCKET}/releases/${GITHUB_SHA}/${ARTIFACT_NAME}" \
            --only-show-errors

      - name: Activate release through SSM
        shell: bash
        run: |
          set -euo pipefail

          ssm_script="$(cat <<REMOTE
          set -euo pipefail

          release_sha="${GITHUB_SHA}"
          artifact_name="${ARTIFACT_NAME}"
          deploy_bucket="${DEPLOY_BUCKET}"
          release_root="/opt/evetools/releases"
          release_dir="\${release_root}/\${release_sha}"
          archive_path="/tmp/\${artifact_name}"

          sudo mkdir -p "\${release_dir}"
          aws s3 cp "s3://\${deploy_bucket}/releases/\${release_sha}/\${artifact_name}" "\${archive_path}" --only-show-errors
          sudo tar -xzf "\${archive_path}" -C "\${release_dir}"
          sudo chmod +x "\${release_dir}/bin/evetools-http-api" "\${release_dir}/bin/sync-public-market-region"
          sudo ln -sfn "\${release_dir}" /opt/evetools/current
          sudo systemctl daemon-reload
          sudo systemctl restart evetools-http-api.service
          sudo systemctl try-restart evetools-market-sync.timer || true
          REMOTE
          )"

          jq -n --arg script "$ssm_script" '{commands: [$script]}' > ssm-parameters.json

          command_id="$(aws ssm send-command \
            --document-name AWS-RunShellScript \
            --instance-ids "${EC2_INSTANCE_ID}" \
            --comment "Deploy EveTools ${GITHUB_SHA}" \
            --parameters file://ssm-parameters.json \
            --query 'Command.CommandId' \
            --output text)"

          echo "SSM command id: ${command_id}"

          for attempt in $(seq 1 60); do
            status="$(aws ssm get-command-invocation \
              --command-id "${command_id}" \
              --instance-id "${EC2_INSTANCE_ID}" \
              --query 'Status' \
              --output text 2>/dev/null || true)"

            echo "SSM status attempt ${attempt}: ${status}"

            case "$status" in
              Success)
                aws ssm get-command-invocation \
                  --command-id "${command_id}" \
                  --instance-id "${EC2_INSTANCE_ID}" \
                  --query 'StandardOutputContent' \
                  --output text
                exit 0
                ;;
              Failed|Cancelled|TimedOut|Cancelling)
                aws ssm get-command-invocation \
                  --command-id "${command_id}" \
                  --instance-id "${EC2_INSTANCE_ID}" \
                  --query '{Status:Status,Stdout:StandardOutputContent,Stderr:StandardErrorContent}' \
                  --output json
                exit 1
                ;;
              Pending|InProgress|Delayed|"")
                sleep 10
                ;;
              *)
                sleep 10
                ;;
            esac
          done

          echo "::error::SSM command did not finish within 10 minutes"
          aws ssm get-command-invocation \
            --command-id "${command_id}" \
            --instance-id "${EC2_INSTANCE_ID}" \
            --query '{Status:Status,Stdout:StandardOutputContent,Stderr:StandardErrorContent}' \
            --output json || true
          exit 1
```

- [ ] **Step 4: Verify YAML parses**

Run:

```bash
ruby -e 'require "yaml"; YAML.load_file(".github/workflows/deploy-prod.yml"); puts "yaml ok"'
```

Expected: PASS and prints `yaml ok`.

- [ ] **Step 5: Verify required workflow capabilities are present**

Run:

```bash
rg -n "id-token: write|configure-aws-credentials@v4|aws s3 cp|aws ssm send-command|get-command-invocation|evetools-http-api|sync-public-market-region" .github/workflows/deploy-prod.yml
```

Expected: PASS and shows matches for all required capabilities.

- [ ] **Step 6: Commit**

Run:

```bash
git add .github/workflows/deploy-prod.yml
git commit -m "ci: deploy backend with s3 ssm oidc"
```

Expected: PASS and creates a commit containing only the workflow.

---

### Task 2: Add AWS Deployment Guide

**Files:**
- Create: `docs/deployment/aws-s3-ssm-oidc.md`
- Modify: `README.md`

- [ ] **Step 1: Write the failing deployment guide existence check**

Run:

```bash
test -f docs/deployment/aws-s3-ssm-oidc.md
```

Expected: FAIL with exit code 1 because the guide does not exist yet.

- [ ] **Step 2: Create the deployment docs directory**

Run:

```bash
mkdir -p docs/deployment
```

Expected: PASS and `docs/deployment` exists.

- [ ] **Step 3: Add the AWS deployment guide**

Create `docs/deployment/aws-s3-ssm-oidc.md` with this exact content:

````markdown
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

创建目录：

```bash
sudo mkdir -p /opt/evetools/releases
sudo chown root:root /opt/evetools /opt/evetools/releases
```

服务端环境变量放在 EC2 本地：

```bash
sudo mkdir -p /etc/evetools
sudo install -m 600 /dev/null /etc/evetools/evetools.env
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
````

- [ ] **Step 4: Add README link**

In `README.md`, add this paragraph immediately after the production sync task command example section and before the sentence that starts with `` `--all-default-regions` 会按顺序同步``:

```markdown
生产后端可以通过 GitHub Actions、S3、SSM 和 GitHub OIDC 部署到固定 EC2 实例。部署准备、IAM 权限、systemd 服务和手动验证步骤见 [AWS S3 + SSM + OIDC 部署指南](docs/deployment/aws-s3-ssm-oidc.md)。
```

- [ ] **Step 5: Verify the guide and README link**

Run:

```bash
rg -n "AWS S3 \\+ SSM \\+ OIDC|EVETOOLS_DATABASE_URL|AWS_ROLE_ARN|evetools-http-api.service|evetools-market-sync.timer" docs/deployment/aws-s3-ssm-oidc.md README.md
```

Expected: PASS and shows matches in both files.

- [ ] **Step 6: Commit**

Run:

```bash
git add docs/deployment/aws-s3-ssm-oidc.md README.md
git commit -m "docs: document aws s3 ssm deployment"
```

Expected: PASS and creates a commit containing only deployment documentation and README link.

---

### Task 3: Final Verification

**Files:**
- Verify: `.github/workflows/deploy-prod.yml`
- Verify: `docs/deployment/aws-s3-ssm-oidc.md`
- Verify: `README.md`

- [ ] **Step 1: Verify workflow YAML syntax**

Run:

```bash
ruby -e 'require "yaml"; YAML.load_file(".github/workflows/deploy-prod.yml"); puts "yaml ok"'
```

Expected: PASS and prints `yaml ok`.

- [ ] **Step 2: Verify workflow deploy contract**

Run:

```bash
rg -n "branches:|main|master|id-token: write|AWS_ROLE_ARN|DEPLOY_BUCKET|EC2_INSTANCE_ID|aws s3 cp|aws ssm send-command|systemctl restart evetools-http-api.service" .github/workflows/deploy-prod.yml
```

Expected: PASS and shows the branch triggers, OIDC permission, required configuration names, S3 upload, SSM deploy, and service restart.

- [ ] **Step 3: Verify docs cover AWS and EC2 contracts**

Run:

```bash
rg -n "OIDC|AmazonSSMManagedInstanceCore|s3:GetObject|s3:PutObject|ssm:SendCommand|/opt/evetools/current|/etc/evetools/evetools.env|手动回滚" docs/deployment/aws-s3-ssm-oidc.md
```

Expected: PASS and shows all operational contract topics.

- [ ] **Step 4: Run backend tests**

Run:

```bash
cargo test -p evetools-domain -p evetools-esi -p evetools-db -p evetools-api -p evetools-http-api -p evetools-worker
```

Expected: PASS. Postgres integration tests that require `EVETOOLS_TEST_DATABASE_URL` may skip according to existing project behavior.

- [ ] **Step 5: Build production binaries**

Run:

```bash
cargo build --release -p evetools-http-api --bin evetools-http-api
cargo build --release -p evetools-worker --bin sync-public-market-region
```

Expected: PASS and creates:

```text
target/release/evetools-http-api
target/release/sync-public-market-region
```

- [ ] **Step 6: Verify release package shape locally**

Run:

```bash
rm -rf /tmp/evetools-deploy-check
mkdir -p /tmp/evetools-deploy-check/dist/bin
cp target/release/evetools-http-api /tmp/evetools-deploy-check/dist/bin/evetools-http-api
cp target/release/sync-public-market-region /tmp/evetools-deploy-check/dist/bin/sync-public-market-region
tar -C /tmp/evetools-deploy-check/dist -czf /tmp/evetools-deploy-check/evetools-check.tar.gz bin
tar -tzf /tmp/evetools-deploy-check/evetools-check.tar.gz
```

Expected: PASS and prints:

```text
bin/
bin/evetools-http-api
bin/sync-public-market-region
```

- [ ] **Step 7: Check repository diff**

Run:

```bash
git status --short
```

Expected: PASS and prints no uncommitted changes.
