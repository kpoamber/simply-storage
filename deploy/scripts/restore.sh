#!/usr/bin/env bash
# PostgreSQL/Citus restore script for Innovare Storage
# Restores database from backup, handles coordinator + workers for Citus profiles
#
# Usage: ./restore.sh --file /backups/backup_small_20260316_020000.tar.gz
#        ./restore.sh --date 20260316  (finds latest backup for that date)
#        ./restore.sh --pitr                              (replay all WAL)
#        ./restore.sh --pitr --target-time '2026-03-17 14:30:00+00'
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
PITR_MODE=false
PITR_TARGET=""

# --- Parse arguments ---
while [[ $# -gt 0 ]]; do
    case "$1" in
        --file) BACKUP_FILE="$2"; shift 2 ;;
        --date) BACKUP_DATE="$2"; shift 2 ;;
        --profile) PROFILE="$2"; shift 2 ;;
        --pitr) PITR_MODE=true; shift ;;
        --target-time) PITR_TARGET="$2"; shift 2 ;;
        *) echo "Unknown argument: $1"; exit 1 ;;
    esac
done

# --- Determine compose files ---
COMPOSE_BASE="${DEPLOY_DIR}/docker-compose.prod.yml"
COMPOSE_PROFILE="${DEPLOY_DIR}/docker-compose.${PROFILE}.yml"
COMPOSE_CMD="docker compose --env-file ${DEPLOY_DIR}/.env -f ${COMPOSE_BASE} -f ${COMPOSE_PROFILE} -p ${COMPOSE_PROJECT}"

# --- PITR Restore Mode ---
if [[ "${PITR_MODE}" = true ]]; then
    BASEBACKUP_DIR="${BACKUP_DIR}/basebackups"
    WAL_DIR="${BACKUP_DIR}/wal"

    echo "=== Point-in-Time Recovery ==="
    echo "Target: ${PITR_TARGET:-latest (replay all available WAL)}"
    echo "Profile: ${PROFILE}"

    if [[ "${PROFILE}" != "small" ]]; then
        echo ""
        echo "WARNING: PITR restores only the coordinator. Worker data must be restored"
        echo "  separately using: ./restore.sh --file <backup.tar.gz> --profile ${PROFILE}"
    fi
    echo ""

    # Find latest base backup
    LATEST_BASE="$(find "${BASEBACKUP_DIR}" -maxdepth 1 -type d -name "basebackup_*" 2>/dev/null | sort -r | head -1)"
    if [[ -z "${LATEST_BASE}" ]]; then
        echo "ERROR: No base backup found in ${BASEBACKUP_DIR}"
        echo "  Run: ./basebackup.sh first"
        exit 1
    fi
    echo "Using base backup: $(basename "${LATEST_BASE}")"

    WAL_COUNT="$(find "${WAL_DIR}" -maxdepth 1 -type f -name "0000*" 2>/dev/null | wc -l)"
    echo "WAL files available: ${WAL_COUNT}"
    echo ""

    # [1] Stop all containers
    echo "[1] Stopping all containers..."
    ${COMPOSE_CMD} down
    echo ""

    # [2] Remove old postgres data volume
    echo "[2] Removing old postgres data volume..."
    docker volume rm "${COMPOSE_PROJECT}_postgres_coordinator_data" 2>/dev/null || true
    echo ""

    # [3] Restore base backup to new volume
    echo "[3] Restoring base backup..."
    docker volume create "${COMPOSE_PROJECT}_postgres_coordinator_data"
    docker run --rm \
        -v "${COMPOSE_PROJECT}_postgres_coordinator_data:/var/lib/postgresql/data" \
        -v "${LATEST_BASE}:/backup:ro" \
        alpine:3.21 sh -c 'cd /var/lib/postgresql/data && tar xzf /backup/base.tar.gz'
    echo ""

    # [4] Configure recovery
    echo "[4] Configuring WAL recovery..."
    RESTORE_CONF="restore_command = 'cp /backups/wal/%f %p'"
    if [[ -n "${PITR_TARGET}" ]]; then
        RESTORE_CONF="${RESTORE_CONF}
recovery_target_time = '${PITR_TARGET}'
recovery_target_action = 'promote'"
    fi

    docker run --rm \
        -v "${COMPOSE_PROJECT}_postgres_coordinator_data:/var/lib/postgresql/data" \
        alpine:3.21 sh -c "
            touch /var/lib/postgresql/data/recovery.signal
            echo \"${RESTORE_CONF}\" >> /var/lib/postgresql/data/postgresql.auto.conf
        "
    echo ""

    # [5] Start postgres in recovery mode
    echo "[5] Starting postgres (recovery mode)..."
    ${COMPOSE_CMD} up -d postgres
    echo "  Waiting for recovery to complete..."
    for attempt in $(seq 1 120); do
        if docker exec "${COMPOSE_PROJECT}-postgres-1" pg_isready -U "${POSTGRES_USER}" -d "${POSTGRES_DB}" > /dev/null 2>&1; then
            IN_RECOVERY="$(docker exec "${COMPOSE_PROJECT}-postgres-1" psql -U "${POSTGRES_USER}" -d "${POSTGRES_DB}" -tAc "SELECT pg_is_in_recovery();" 2>/dev/null || echo "t")"
            if [[ "${IN_RECOVERY}" = "f" ]]; then
                echo "  Recovery complete!"
                break
            fi
        fi
        if [[ ${attempt} -eq 120 ]]; then
            echo "  WARNING: Recovery not complete after 10 minutes."
        fi
        sleep 5
    done
    echo ""

    # [6] Start remaining services
    echo "[6] Starting app containers..."
    ${COMPOSE_CMD} up -d
    echo ""

    # [7] Health check
    echo "[7] Running health check..."
    HEALTHY=false
    for attempt in $(seq 1 60); do
        if curl -sf "${HEALTH_URL}" > /dev/null 2>&1; then
            HEALTHY=true; break
        fi
        sleep 2
    done
    if [[ "${HEALTHY}" = true ]]; then
        echo "  Health check passed!"
    else
        echo "WARNING: Health check failed. Check: ${COMPOSE_CMD} logs app"
    fi

    echo ""
    echo "=== PITR Restore Complete ==="
    echo "Base backup: $(basename "${LATEST_BASE}")"
    echo "Target: ${PITR_TARGET:-latest}"
    exit 0
fi

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

# --- Helper: restore dump to container ---
restore_container() {
    local container="$1"
    local dump_file="$2"
    echo "  Restoring ${dump_file} -> ${container}..."
    set +e
    docker exec -i "${container}" pg_restore \
        -U "${POSTGRES_USER}" \
        -d "${POSTGRES_DB}" \
        --no-owner \
        --no-privileges \
        --clean \
        --if-exists \
        < "${dump_file}"
    local rc=$?
    set -e
    if [[ ${rc} -eq 0 ]]; then
        echo "  Done."
    elif [[ ${rc} -eq 1 ]]; then
        echo "  WARNING: pg_restore completed with warnings (exit code 1). This is usually safe."
    else
        echo "  ERROR: pg_restore failed with exit code ${rc}"
        return ${rc}
    fi
}

# --- Extract backup ---
echo "[1] Extracting backup..."
TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TEMP_DIR}"' EXIT
tar -xzf "${BACKUP_FILE}" -C "${TEMP_DIR}"
BACKUP_NAME="$(ls -1 "${TEMP_DIR}" | head -1)"
EXTRACTED="${TEMP_DIR}/${BACKUP_NAME}"
echo "  Extracted to: ${EXTRACTED}"

# Warn if backup profile doesn't match target profile
BACKUP_PROFILE="$(echo "${BACKUP_NAME}" | sed -n 's/backup_\([a-z]*\)_.*/\1/p')"
if [[ -n "${BACKUP_PROFILE}" && "${BACKUP_PROFILE}" != "${PROFILE}" ]]; then
    echo "  WARNING: Backup was created with profile '${BACKUP_PROFILE}' but restoring to profile '${PROFILE}'."
    echo "  This may result in incomplete data restoration if worker counts differ."
fi
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
