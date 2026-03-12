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

## Cloud Deployment (Hetzner / DigitalOcean)

### Prerequisites

1. Docker image pushed to GHCR (automatic via GitHub Actions on push to `main`)
2. A GHCR personal access token with `read:packages` scope
3. A Hetzner Cloud or DigitalOcean account

### One-Click Deploy (New Server)

#### Option A: Cloud-Init (Automated)

1. Edit `deploy/cloud-init.yml` and set your GHCR credentials and image repo
2. Create a new server via Hetzner Cloud Console or DigitalOcean:
   - Select Ubuntu 22.04+ or Debian 12+
   - Paste the cloud-init YAML as user-data
3. The server will automatically:
   - Install Docker
   - Authenticate to GHCR
   - Pull and start the Innovare Storage image
   - Create a systemd service for auto-restart

#### Option B: Deploy Script (Manual)

SSH into the server and run:

```bash
# First-time setup
curl -O https://raw.githubusercontent.com/yourorg/innovare-storage/main/deploy/deploy.sh
chmod +x deploy.sh

# Standalone deploy
IMAGE_REPO=ghcr.io/yourorg/innovare-storage:latest ./deploy.sh

# Or join an existing cluster
IMAGE_REPO=ghcr.io/yourorg/innovare-storage:latest ./deploy.sh --join 10.0.0.1
```

### Adding a New Node to an Existing Cluster

```bash
./deploy.sh --join <existing-node-ip>
```

This will:
1. Pull the latest Docker image from GHCR
2. Fetch configuration from the existing node via `GET /api/system/config-export`
3. Write the database URL and HMAC secret to a local config file
4. Start the service container
5. The new instance registers itself and begins sending heartbeats

### Rolling Updates

To update to the latest image:

```bash
./deploy.sh --update
```

Or specify a specific image:

```bash
./deploy.sh --update --image ghcr.io/yourorg/innovare-storage:abc1234
```

### TLS Configuration

For HTTPS support:

1. Place your TLS certificates at a known path:
   - `fullchain.pem` - Certificate chain
   - `privkey.pem` - Private key

2. Mount them in docker-compose.yml (uncomment the certs volume)

3. Uncomment the HTTPS server block in `docker/nginx.conf`

Alternatively, use Let's Encrypt with certbot on the host and mount the generated certs.

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
