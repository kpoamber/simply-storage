#!/usr/bin/env bash
# Deploy script for Innovare Storage on Hetzner Cloud
# Pulls GHCR image, runs pre-deploy backup, starts services, health checks, rollback on failure
#
# Usage: ./deploy.sh --profile small|medium|large [--image-tag latest] [--skip-backup]
# Environment: POSTGRES_USER, POSTGRES_PASSWORD, POSTGRES_DB, GHCR_TOKEN, GHCR_USER
set -euo pipefail

# --- Configuration ---
DEPLOY_DIR="${DEPLOY_DIR:-/opt/innovare-storage}"
PROFILE="${PROFILE:-small}"
IMAGE_TAG="${IMAGE_TAG:-latest}"
COMPOSE_PROJECT="${COMPOSE_PROJECT:-innovare-storage}"
BACKUP_DIR="${BACKUP_DIR:-/backups}"
HEALTH_URL="${HEALTH_URL:-http://localhost:80/health}"
HEALTH_TIMEOUT="${HEALTH_TIMEOUT:-120}"
SKIP_BACKUP="${SKIP_BACKUP:-false}"
GHCR_REGISTRY="${GHCR_REGISTRY:-ghcr.io}"
GHCR_USER="${GHCR_USER:-}"
GHCR_TOKEN="${GHCR_TOKEN:-}"
ROLLBACK_TAG=""

# --- Parse arguments ---
while [[ $# -gt 0 ]]; do
    case "$1" in
        --profile) PROFILE="$2"; shift 2 ;;
        --image-tag) IMAGE_TAG="$2"; shift 2 ;;
        --skip-backup) SKIP_BACKUP=true; shift ;;
        --deploy-dir) DEPLOY_DIR="$2"; shift 2 ;;
        *) echo "Unknown argument: $1"; exit 1 ;;
    esac
done

# --- Compose command helper ---
COMPOSE_BASE="${DEPLOY_DIR}/docker-compose.prod.yml"
COMPOSE_PROFILE="${DEPLOY_DIR}/docker-compose.${PROFILE}.yml"
COMPOSE_CMD="docker compose -f ${COMPOSE_BASE} -f ${COMPOSE_PROFILE} -p ${COMPOSE_PROJECT}"

# --- Validate profile ---
if [[ ! -f "${COMPOSE_PROFILE}" ]]; then
    echo "ERROR: Profile file not found: ${COMPOSE_PROFILE}"
    echo "Valid profiles: small, medium, large"
    exit 1
fi

echo "=== Innovare Storage Deploy ==="
echo "Profile:   ${PROFILE}"
echo "Image tag: ${IMAGE_TAG}"
echo "Deploy dir: ${DEPLOY_DIR}"
echo ""

# --- Step 1: Login to GHCR and pull image ---
echo "[1/6] Pulling image from GHCR..."
if [[ -n "${GHCR_TOKEN}" && -n "${GHCR_USER}" ]]; then
    echo "${GHCR_TOKEN}" | docker login "${GHCR_REGISTRY}" -u "${GHCR_USER}" --password-stdin
fi

# Export IMAGE_TAG for docker compose
export IMAGE_TAG
${COMPOSE_CMD} pull app
echo "  Image pulled successfully."
echo ""

# --- Step 2: Record current image for rollback ---
echo "[2/6] Recording current state for rollback..."
CURRENT_IMAGE="$(docker inspect --format='{{.Config.Image}}' "${COMPOSE_PROJECT}-app-1" 2>/dev/null || echo "")"
if [[ -n "${CURRENT_IMAGE}" ]]; then
    ROLLBACK_TAG="${CURRENT_IMAGE}"
    echo "  Current image: ${ROLLBACK_TAG}"
else
    echo "  No running app container found (fresh deploy)."
fi
echo ""

# --- Step 3: Pre-deploy backup ---
if [[ "${SKIP_BACKUP}" = "false" ]]; then
    echo "[3/6] Running pre-deploy backup..."
    POSTGRES_CONTAINER="${COMPOSE_PROJECT}-postgres-1"
    if docker ps --format '{{.Names}}' | grep -q "^${POSTGRES_CONTAINER}$"; then
        bash "${DEPLOY_DIR}/scripts/backup.sh" --profile "${PROFILE}" --backup-dir "${BACKUP_DIR}" || {
            echo "WARNING: Pre-deploy backup failed. Continuing with deploy..."
        }
    else
        echo "  No running postgres container. Skipping backup (fresh deploy)."
    fi
else
    echo "[3/6] Skipping pre-deploy backup (--skip-backup)."
fi
echo ""

# --- Step 4: Load environment ---
echo "[4/6] Loading environment..."
ENV_FILE="${DEPLOY_DIR}/.env"
if [[ -f "${ENV_FILE}" ]]; then
    echo "  Using env file: ${ENV_FILE}"
else
    echo "WARNING: No .env file found at ${ENV_FILE}"
    echo "  Using environment variables from shell."
fi
echo ""

# --- Step 5: Deploy with docker compose ---
echo "[5/6] Starting services..."
${COMPOSE_CMD} up -d --remove-orphans
echo "  Services started."
echo ""

# --- Step 6: Health check ---
echo "[6/6] Running health check (timeout: ${HEALTH_TIMEOUT}s)..."
HEALTHY=false
ELAPSED=0
while [[ ${ELAPSED} -lt ${HEALTH_TIMEOUT} ]]; do
    if curl -sf "${HEALTH_URL}" > /dev/null 2>&1; then
        HEALTHY=true
        break
    fi
    sleep 2
    ELAPSED=$((ELAPSED + 2))
done

if [[ "${HEALTHY}" = true ]]; then
    echo "  Health check passed after ${ELAPSED}s!"
    echo ""
    echo "=== Deploy Successful ==="
    echo "Profile: ${PROFILE}"
    echo "Image tag: ${IMAGE_TAG}"
    echo ""
    # Show running containers
    ${COMPOSE_CMD} ps
    exit 0
fi

# --- Rollback on failure ---
echo ""
echo "ERROR: Health check failed after ${HEALTH_TIMEOUT}s!"
echo ""

# Show logs for debugging
echo "=== Recent app logs ==="
${COMPOSE_CMD} logs --tail=50 app 2>&1 || true
echo ""

if [[ -n "${ROLLBACK_TAG}" ]]; then
    echo "=== Rolling back to ${ROLLBACK_TAG} ==="

    # Find latest pre-deploy backup
    LATEST_BACKUP="$(find "${BACKUP_DIR}" -maxdepth 1 -name "backup_*.tar.gz" -type f -print 2>/dev/null | sort -r | head -1 || true)"

    # Restore from pre-deploy backup
    if [[ -n "${LATEST_BACKUP}" ]]; then
        echo "Restoring from pre-deploy backup: ${LATEST_BACKUP}"
        bash "${DEPLOY_DIR}/scripts/restore.sh" --file "${LATEST_BACKUP}" --profile "${PROFILE}" || {
            echo "ERROR: Rollback restore failed!"
        }
    fi

    # Roll back image
    echo "Rolling back image..."
    export IMAGE_TAG="${ROLLBACK_TAG##*:}"
    ${COMPOSE_CMD} up -d --remove-orphans
    echo ""

    # Health check after rollback
    echo "Health check after rollback..."
    ROLLBACK_HEALTHY=false
    ELAPSED=0
    while [[ ${ELAPSED} -lt 60 ]]; do
        if curl -sf "${HEALTH_URL}" > /dev/null 2>&1; then
            ROLLBACK_HEALTHY=true
            break
        fi
        sleep 2
        ELAPSED=$((ELAPSED + 2))
    done

    if [[ "${ROLLBACK_HEALTHY}" = true ]]; then
        echo "  Rollback succeeded. Service is healthy."
    else
        echo "  WARNING: Rollback health check also failed. Manual intervention required."
        echo "  Check: ${COMPOSE_CMD} logs app"
    fi
else
    echo "No previous image found for rollback. Manual intervention required."
    echo "Check: ${COMPOSE_CMD} logs app"
fi

exit 1
