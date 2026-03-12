#!/usr/bin/env bash
set -euo pipefail

# ─── Innovare Storage Deploy Script ─────────────────────────────────────────
#
# Deploys or updates Innovare Storage on a server.
# Supports joining an existing cluster by syncing configuration.
#
# Usage:
#   ./deploy.sh                              # Standalone deploy (uses local config)
#   ./deploy.sh --join <existing-node-ip>    # Join existing cluster
#   ./deploy.sh --update                     # Pull latest image and restart
#
# Environment variables:
#   IMAGE_REPO    - Docker image (default: ghcr.io/yourorg/innovare-storage:latest)
#   CONTAINER     - Container name (default: innovare-storage)
#   CONFIG_DIR    - Config directory (default: /opt/innovare-storage/config)
#   DATA_DIR      - Data directory (default: /opt/innovare-storage/data)

IMAGE_REPO="${IMAGE_REPO:-ghcr.io/yourorg/innovare-storage:latest}"
CONTAINER="${CONTAINER:-innovare-storage}"
CONFIG_DIR="${CONFIG_DIR:-/opt/innovare-storage/config}"
DATA_DIR="${DATA_DIR:-/opt/innovare-storage/data}"

JOIN_IP=""
UPDATE_ONLY=false

# ─── Parse arguments ────────────────────────────────────────────────────────

while [[ $# -gt 0 ]]; do
    case "$1" in
        --join)
            JOIN_IP="$2"
            shift 2
            ;;
        --update)
            UPDATE_ONLY=true
            shift
            ;;
        --image)
            IMAGE_REPO="$2"
            shift 2
            ;;
        --help|-h)
            echo "Usage: $0 [--join <ip>] [--update] [--image <image>]"
            echo ""
            echo "Options:"
            echo "  --join <ip>      Join existing cluster by syncing config from <ip>"
            echo "  --update         Pull latest image and restart (rolling update)"
            echo "  --image <image>  Override Docker image (default: $IMAGE_REPO)"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

# ─── Functions ──────────────────────────────────────────────────────────────

log() {
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*"
}

pull_image() {
    log "Pulling latest image: $IMAGE_REPO"
    docker pull "$IMAGE_REPO"
}

stop_container() {
    if docker ps -q -f "name=$CONTAINER" | grep -q .; then
        log "Stopping existing container: $CONTAINER"
        docker stop "$CONTAINER" 2>/dev/null || true
        docker rm "$CONTAINER" 2>/dev/null || true
    fi
}

start_container() {
    log "Starting container: $CONTAINER"
    docker run -d \
        --name "$CONTAINER" \
        --network host \
        --restart unless-stopped \
        -v "$DATA_DIR:/app/data" \
        -v "$CONFIG_DIR:/app/config" \
        -e RUST_LOG="${RUST_LOG:-info}" \
        "$IMAGE_REPO"
    log "Container started successfully"
}

wait_for_health() {
    local max_attempts=30
    local attempt=0
    log "Waiting for health check..."
    while [ $attempt -lt $max_attempts ]; do
        if curl -sf http://localhost:8080/health > /dev/null 2>&1; then
            log "Service is healthy"
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 2
    done
    log "ERROR: Service failed health check after ${max_attempts} attempts"
    return 1
}

sync_config_from_node() {
    local source_ip="$1"
    log "Syncing configuration from existing node at $source_ip..."

    mkdir -p "$CONFIG_DIR"

    local config_json
    config_json=$(curl -sf "http://${source_ip}:8080/api/system/config-export") || {
        log "ERROR: Failed to fetch config from $source_ip"
        exit 1
    }

    echo "$config_json" > "$CONFIG_DIR/cluster-config.json"

    # Extract database URL and HMAC secret from exported config to create a TOML config
    local db_url hmac_secret
    db_url=$(echo "$config_json" | jq -r '.config.database.url // empty')
    hmac_secret=$(echo "$config_json" | jq -r '.config.storage.hmac_secret // empty')

    if [ -n "$db_url" ] && [ -n "$hmac_secret" ]; then
        cat > "$CONFIG_DIR/default.toml" <<EOF
[database]
url = "$db_url"

[storage]
hmac_secret = "$hmac_secret"
EOF
        log "Config file written to $CONFIG_DIR/default.toml"
    else
        log "WARNING: Could not extract DB URL or HMAC secret from exported config"
    fi

    log "Configuration synced successfully"
}

# ─── Main ───────────────────────────────────────────────────────────────────

mkdir -p "$DATA_DIR/temp" "$CONFIG_DIR"

# Pull latest image
pull_image

# If joining an existing cluster, sync config first
if [ -n "$JOIN_IP" ]; then
    sync_config_from_node "$JOIN_IP"
fi

# Stop old container and start new one
stop_container
start_container

# Wait for service to become healthy
wait_for_health

log "Deploy complete"
