#!/usr/bin/env bash
# PostgreSQL/Citus restore script for Innovare Storage
# Restores database from backup, handles coordinator + workers for Citus profiles
#
# Usage: ./restore.sh --file /backups/backup_small_20260316_020000.tar.gz
#        ./restore.sh --date 20260316  (finds latest backup for that date)
#
# Environment: POSTGRES_USER, POSTGRES_DB, COMPOSE_PROJECT, DEPLOY_DIR, PROFILE
set -euo pipefail

# --- Configuration ---
BACKUP_DIR="${BACKUP_DIR:-/backups}"
POSTGRES_USER="${POSTGRES_USER:-innovare}"
POSTGRES_DB="${POSTGRES_DB:-innovare_storage}"
COMPOSE_PROJECT="${COMPOSE_PROJECT:-innovare-storage}"
DEPLOY_DIR="${DEPLOY_DIR:-/opt/innovare-storage}"
PROFILE="${PROFILE:-small}"
HEALTH_URL="${HEALTH_URL:-http://localhost:80/health}"
BACKUP_FILE=""
BACKUP_DATE=""

# --- Parse arguments ---
while [[ $# -gt 0 ]]; do
    case "$1" in
        --file) BACKUP_FILE="$2"; shift 2 ;;
        --date) BACKUP_DATE="$2"; shift 2 ;;
        --profile) PROFILE="$2"; shift 2 ;;
        *) echo "Unknown argument: $1"; exit 1 ;;
    esac
done

# --- Resolve backup file ---
if [[ -n "${BACKUP_DATE}" && -z "${BACKUP_FILE}" ]]; then
    echo "Searching for latest backup on date ${BACKUP_DATE}..."
    BACKUP_FILE="$(find "${BACKUP_DIR}" -maxdepth 1 -name "backup_*_${BACKUP_DATE}_*.tar.gz" -type f -print 2>/dev/null | sort -r | head -1 || true)"
    if [[ -z "${BACKUP_FILE}" ]]; then
        echo "ERROR: No backup found for date ${BACKUP_DATE}"
        exit 1
    fi
fi

if [[ -z "${BACKUP_FILE}" ]]; then
    echo "ERROR: Must specify --file or --date"
    echo "Usage: ./restore.sh --file /backups/backup_small_20260316.tar.gz"
    echo "       ./restore.sh --date 20260316"
    exit 1
fi

if [[ ! -f "${BACKUP_FILE}" ]]; then
    echo "ERROR: Backup file not found: ${BACKUP_FILE}"
    exit 1
fi

echo "=== Innovare Storage Restore ==="
echo "Backup file: ${BACKUP_FILE}"
echo "Profile: ${PROFILE}"
echo ""

# --- Determine compose files ---
COMPOSE_BASE="${DEPLOY_DIR}/docker-compose.prod.yml"
COMPOSE_PROFILE="${DEPLOY_DIR}/docker-compose.${PROFILE}.yml"
COMPOSE_CMD="docker compose -f ${COMPOSE_BASE} -f ${COMPOSE_PROFILE} -p ${COMPOSE_PROJECT}"

# --- Helper: restore dump to container ---
restore_container() {
    local container="$1"
    local dump_file="$2"
    echo "  Restoring ${dump_file} -> ${container}..."
    docker exec -i "${container}" pg_restore \
        -U "${POSTGRES_USER}" \
        -d "${POSTGRES_DB}" \
        --no-owner \
        --no-privileges \
        --clean \
        --if-exists \
        < "${dump_file}"
    echo "  Done."
}

# --- Extract backup ---
echo "[1] Extracting backup..."
TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TEMP_DIR}"' EXIT
tar -xzf "${BACKUP_FILE}" -C "${TEMP_DIR}"
BACKUP_NAME="$(ls "${TEMP_DIR}")"
EXTRACTED="${TEMP_DIR}/${BACKUP_NAME}"
echo "  Extracted to: ${EXTRACTED}"
echo ""

# --- Stop app containers (keep database running) ---
echo "[2] Stopping app containers..."
${COMPOSE_CMD} stop app nginx || true
echo "  App containers stopped."
echo ""

# --- Wait for postgres to be ready ---
echo "[3] Waiting for postgres to be ready..."
COORDINATOR="${COMPOSE_PROJECT}-postgres-1"
for attempt in $(seq 1 30); do
    if docker exec "${COORDINATOR}" pg_isready -U "${POSTGRES_USER}" -d "${POSTGRES_DB}" > /dev/null 2>&1; then
        echo "  PostgreSQL is ready."
        break
    fi
    if [[ ${attempt} -eq 30 ]]; then
        echo "ERROR: PostgreSQL not ready after 30 attempts"
        exit 1
    fi
    sleep 1
done
echo ""

# --- Restore coordinator ---
echo "[4] Restoring coordinator..."
restore_container "${COORDINATOR}" "${EXTRACTED}/coordinator.dump"
echo ""

# --- Restore workers if present ---
WORKER_DUMPS="$(ls "${EXTRACTED}"/worker_*.dump 2>/dev/null || true)"
if [[ -n "${WORKER_DUMPS}" ]]; then
    WORKER_NUM=0
    for dump_file in "${EXTRACTED}"/worker_*.dump; do
        WORKER_NUM=$((WORKER_NUM + 1))
        WORKER="${COMPOSE_PROJECT}-postgres-worker-${WORKER_NUM}-1"
        echo "[4.${WORKER_NUM}] Restoring worker ${WORKER_NUM}..."
        restore_container "${WORKER}" "${dump_file}"
        echo ""
    done
fi

# --- Start app containers ---
echo "[5] Starting app containers..."
${COMPOSE_CMD} up -d app nginx
echo "  Containers started."
echo ""

# --- Health check ---
echo "[6] Running health check..."
HEALTHY=false
for attempt in $(seq 1 60); do
    if curl -sf "${HEALTH_URL}" > /dev/null 2>&1; then
        HEALTHY=true
        break
    fi
    sleep 2
done

if [[ "${HEALTHY}" = true ]]; then
    echo "  Health check passed!"
else
    echo "WARNING: Health check failed after 120 seconds. App may still be starting."
    echo "  Check: ${COMPOSE_CMD} logs app"
fi

echo ""
echo "=== Restore Complete ==="
echo "Backup: ${BACKUP_FILE}"
echo "Profile: ${PROFILE}"
