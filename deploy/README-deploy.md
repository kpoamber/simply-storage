# Innovare Storage - Deployment Guide

## Local Development with Docker Compose

Start the full stack locally:

```bash
docker-compose up --build
```

Scale app instances:

```bash
docker-compose up --build --scale app=3
```

This starts:
- **nginx** - Load balancer on ports 80/443
- **app** (2 replicas by default) - Innovare Storage instances
- **postgres** - PostgreSQL + Citus coordinator
- **postgres-worker-1**, **postgres-worker-2** - Citus workers

Access the service at `http://localhost`.

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `APP_REPLICAS` | `2` | Number of app instances |
| `HMAC_SECRET` | `change-me-in-production` | HMAC secret for signed URLs |
| `RUST_LOG` | `info` | Log level (trace, debug, info, warn, error) |

## Cloud Deployment (Hetzner / Windows Server)

### Prerequisites

1. Docker image pushed to GHCR (automatic via GitHub Actions on push to `main`)
2. SSH access to the target server
3. Docker and Docker Compose installed on the target server

### Option A: Terraform (Hetzner)

```bash
cd terraform
terraform init
terraform plan -var-file=tfvars/small.tfvars
terraform apply -var-file=tfvars/small.tfvars
```

The Terraform setup provisions a server with Docker, creates a deploy user, mounts a backup volume, and configures backup cron via `terraform/cloud-init.yml`.

### Option B: Deploy Script (Manual)

```bash
# Deploy with small profile
deploy/scripts/deploy.sh --profile small --image-tag latest --deploy-dir /opt/innovare-storage

# Deploy with medium profile (Citus + 2 workers)
deploy/scripts/deploy.sh --profile medium --image-tag v1.0.0

# Skip pre-deploy backup
deploy/scripts/deploy.sh --profile small --skip-backup
```

### Option C: GitHub Actions (Automated)

Use the `deploy-hetzner.yml` or `deploy-windows.yml` workflows via GitHub Actions. Enable auto-deploy by setting repository variables `AUTO_DEPLOY_HETZNER=true` or `AUTO_DEPLOY_WINDOWS=true`.

### TLS Configuration

TLS is managed automatically via Certbot/Let's Encrypt:
1. Set the `DOMAIN` environment variable in your `.env` file
2. On first deploy, a self-signed placeholder certificate is generated
3. Run certbot to obtain a real certificate: `docker compose exec certbot certbot certonly --webroot -w /var/www/certbot -d yourdomain.com`
4. The certbot container auto-renews certificates every 12 hours

## CI/CD Pipeline

### GitHub Actions Workflows

| Workflow | Trigger | Description |
|----------|---------|-------------|
| `ci.yml` | Push (any branch), PR to main | Backend (clippy, tests) + frontend (lint, build) checks |
| `build-push.yml` | Push to main, tags `v*` | Build Docker image, push to GHCR |
| `deploy-hetzner.yml` | Manual (workflow_dispatch) | Deploy to Hetzner Cloud server via SSH |
| `deploy-windows.yml` | Manual (workflow_dispatch) | Deploy to Windows Server via SSH |
| `backup.yml` | Daily 2:00 UTC, manual | Database backup (PostgreSQL/Citus) |
| `restore.yml` | Manual | Restore database from backup |

### Required GitHub Secrets

| Secret | Used By | Description |
|--------|---------|-------------|
| `HETZNER_SSH_KEY` | deploy-hetzner, backup, restore | SSH private key for Hetzner server |
| `HETZNER_HOST` | deploy-hetzner, backup, restore | Hetzner server IP or hostname |
| `DEPLOY_USER` | deploy-hetzner | SSH user on Hetzner (default: `deploy`) |
| `WINDOWS_SSH_KEY` | deploy-windows, backup, restore | SSH private key for Windows server |
| `WINDOWS_HOST` | deploy-windows, backup, restore | Windows server IP or hostname |
| `WINDOWS_USER` | deploy-windows, backup, restore | SSH user on Windows server |
| `POSTGRES_USER` | backup, restore | PostgreSQL user (default: `innovare`) |
| `POSTGRES_DB` | backup, restore | PostgreSQL database (default: `innovare_storage`) |
| `BACKUP_WEBHOOK_URL` | backup | Optional webhook URL for backup failure notifications |

### Production Deployment Profiles

Use compose overlay files for different server sizes:

```bash
# From the deploy/ directory:

# Small (1 app replica, standalone Postgres)
docker compose -f docker-compose.prod.yml -f docker-compose.small.yml up -d

# Medium (2 app replicas, Citus with 2 workers)
docker compose -f docker-compose.prod.yml -f docker-compose.medium.yml up -d

# Large (4 app replicas, Citus with 4 workers)
docker compose -f docker-compose.prod.yml -f docker-compose.large.yml up -d
```

Copy `.env.example` to `.env` and configure before deploying.

### Backup & Restore

```bash
# Manual backup
./scripts/backup.sh --profile small --backup-dir /backups

# Restore from specific date
./scripts/restore.sh --date 20260316 --profile small

# Restore from specific file
./scripts/restore.sh --file /backups/backup-20260316-020000.tar.gz --profile small
```

## Monitoring

### Health Check

```bash
curl http://localhost/health
# {"status":"ok","service":"innovare-storage"}
```

### Active Nodes

```bash
curl http://localhost/api/system/nodes
```

Returns all nodes that have sent a heartbeat within the last 90 seconds.

### System Stats

```bash
curl http://localhost/api/system/stats
```

## Architecture

```
                    ┌─────────┐
                    │  nginx   │ :80/:443
                    └────┬─────┘
                         │ least_conn
              ┌──────────┼──────────┐
              ▼          ▼          ▼
         ┌────────┐ ┌────────┐ ┌────────┐
         │ app-1  │ │ app-2  │ │ app-N  │  :8080
         └───┬────┘ └───┬────┘ └───┬────┘
             │          │          │
             └──────────┼──────────┘
                        ▼
              ┌──────────────────┐
              │   PostgreSQL     │
              │  (Citus coord)  │
              └───┬─────────┬───┘
                  ▼         ▼
            ┌─────────┐ ┌─────────┐
            │ worker-1│ │ worker-2│
            └─────────┘ └─────────┘
```

Each app instance:
- Runs background sync workers (distributed locking via PostgreSQL advisory locks)
- Registers itself in the `nodes` table and sends heartbeats every 30s
- Serves both the REST API and the admin frontend
