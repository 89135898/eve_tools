# S3 SSM OIDC Production Deploy Design

Date: 2026-05-29

## Purpose

Define the first production deployment path for EveTools backend binaries using GitHub Actions, AWS S3, AWS Systems Manager, and GitHub OIDC.

The goal is to build Linux release binaries on GitHub-hosted runners, publish an immutable release artifact to S3, and activate that artifact on a fixed AWS EC2 instance through SSM Run Command. The deployment must not require SSH access or long-lived AWS access keys in GitHub.

## Confirmed Scope

- Add a GitHub Actions production deploy workflow.
- Build the server-side Rust binaries:
  - `evetools-http-api`
  - `sync-public-market-region`
- Upload a compressed release artifact to S3 under a commit SHA path.
- Use GitHub OIDC to assume an AWS IAM role.
- Use SSM Run Command to activate the release on one fixed EC2 `instance-id`.
- Restart the HTTP API systemd service after activation.
- Keep deployment configuration in GitHub repository variables or secrets.
- Document required AWS resources, IAM permissions, and EC2 filesystem/systemd expectations.

## Out Of Scope

- SSH-based deployment.
- AWS CodeDeploy.
- ECS, EKS, Lambda, or container image deployment.
- Blue/green traffic shifting.
- Multi-instance deployments.
- Automatic database backup or migration orchestration.
- Provisioning AWS resources with Terraform or CloudFormation.
- Creating Supabase projects or managing Supabase secrets.
- Alert delivery integrations.

## Deployment Model

Use this release path:

```text
GitHub Actions
  |
  | OIDC assume role
  v
AWS IAM role
  |
  | upload release artifact
  v
S3 bucket
  |
  | SSM Run Command to fixed instance-id
  v
EC2 instance
  |
  | download, unpack, switch symlink, restart systemd
  v
evetools-http-api
```

GitHub Actions owns build and deployment orchestration. EC2 owns runtime configuration and process supervision.

## GitHub Configuration

The workflow reads these GitHub repository variables or secrets:

- `AWS_REGION`
- `AWS_ROLE_ARN`
- `DEPLOY_BUCKET`
- `EC2_INSTANCE_ID`

`AWS_ROLE_ARN`, `DEPLOY_BUCKET`, and `EC2_INSTANCE_ID` can be repository variables if the repository is private and the values are not sensitive in the user's threat model. They may be secrets if preferred.

No static AWS access key should be stored in GitHub. Authentication must use GitHub's OIDC token and `aws-actions/configure-aws-credentials`.

## AWS IAM Design

Create an IAM OIDC provider for `https://token.actions.githubusercontent.com`.

Create one deployment role trusted by the GitHub repository. The trust policy should restrict:

- `token.actions.githubusercontent.com:aud` to `sts.amazonaws.com`.
- `token.actions.githubusercontent.com:sub` to the intended repository and branch or environment.

The deployment role needs only:

- `s3:PutObject` for `arn:aws:s3:::<deploy-bucket>/releases/*`.
- `ssm:SendCommand` for the fixed EC2 instance and the `AWS-RunShellScript` document.
- `ssm:GetCommandInvocation` for deployment status polling.

The EC2 instance profile needs:

- `s3:GetObject` for `arn:aws:s3:::<deploy-bucket>/releases/*`.
- Normal SSM managed-instance permissions, typically through `AmazonSSMManagedInstanceCore`.

## EC2 Runtime Contract

The target EC2 instance must be visible as a managed instance in Systems Manager before deployment.

The instance must have:

- AWS CLI available to download the release artifact from S3.
- `tar`.
- `systemd`.
- A writable release root at `/opt/evetools/releases`.
- A runtime symlink at `/opt/evetools/current`.
- A service named `evetools-http-api.service`.

The HTTP service should load server-only environment variables from an EC2-local file, for example:

```text
/etc/evetools/evetools.env
```

This file contains values such as:

- `EVETOOLS_DATABASE_URL`
- `EVETOOLS_HTTP_ADDR`

Database credentials must stay on EC2 or in an AWS secret retrieval path. They must not be packaged into the release artifact or placed in GitHub Actions variables.

## Release Artifact

The workflow builds release binaries on `ubuntu-24.04` using Rust `1.82.0`.

The release archive layout is:

```text
bin/evetools-http-api
bin/sync-public-market-region
```

The artifact key is:

```text
releases/<git-sha>/evetools-<git-sha>-linux-x86_64.tar.gz
```

Artifacts are immutable by commit SHA. The EC2 activation step unpacks each release into:

```text
/opt/evetools/releases/<git-sha>
```

Then it atomically updates:

```text
/opt/evetools/current -> /opt/evetools/releases/<git-sha>
```

## Deployment Command

The SSM command on EC2 must:

1. Create `/opt/evetools/releases/<git-sha>`.
2. Download the release artifact from S3.
3. Unpack it into the release directory.
4. Ensure binaries are executable.
5. Atomically switch `/opt/evetools/current`.
6. Run `systemctl daemon-reload`.
7. Restart `evetools-http-api.service`.
8. Optionally run `systemctl try-restart evetools-market-sync.timer` without failing when the timer does not exist.

The workflow should poll `ssm get-command-invocation` until the command reaches `Success`, `Failed`, `Cancelled`, or `TimedOut`.

## Trigger Policy

The production deployment workflow should run on:

- Pushes to `main`.
- Manual `workflow_dispatch`.

Use a concurrency group:

```text
deploy-prod
```

Do not cancel an in-progress production deployment automatically.

## Testing And Verification

The workflow must run these checks before deployment:

```bash
cargo test -p evetools-domain -p evetools-esi -p evetools-db -p evetools-api -p evetools-http-api -p evetools-worker
cargo build --release -p evetools-http-api --bin evetools-http-api
cargo build --release -p evetools-worker --bin sync-public-market-region
```

After SSM activation succeeds, the workflow should verify command success through SSM status polling. It should not require public HTTP access to the EC2 instance, because the service may be behind a private network, load balancer, reverse proxy, or security group.

Runtime verification after deployment is:

```bash
systemctl status evetools-http-api
curl http://127.0.0.1:8080/health
curl http://127.0.0.1:8080/ready
```

The curl checks may be run manually on the instance or added to the SSM command later once the final listen address and local health policy are stable.

## Failure Behavior

If build or tests fail, no artifact is uploaded and no SSM command is sent.

If upload fails, no SSM command is sent.

If SSM command fails, the GitHub Actions job fails. The previous release directory remains on disk, and `/opt/evetools/current` points to whichever release was active before the failed switch unless the failure occurred after symlink update.

This first version does not implement automatic rollback. Manual rollback is:

```bash
sudo ln -sfn /opt/evetools/releases/<previous-sha> /opt/evetools/current
sudo systemctl restart evetools-http-api
```

## Security Notes

- Do not store AWS access keys in GitHub.
- Do not store `EVETOOLS_DATABASE_URL` in GitHub.
- Keep S3 permissions scoped to the release prefix.
- Keep SSM permissions scoped to the fixed EC2 instance.
- Prefer branch or environment restrictions in the OIDC trust policy.
- Do not open SSH solely for deployment.

