#!/usr/bin/env bash
# PostgreSQL base backup for PITR support
# Creates a pg_basebackup of the coordinator for use with WAL archiving
#
# Usage: ./basebackup.sh [--backup-dir /path]
# Environment: POSTGRES_USER, POSTGRES_DB, COMPOSE_PROJECT
set -euo pipefail

# --- Configuration ---
BACKUP_DIR="${BACKUP_DIR:-/backups}"
BASEBACKUP_DIR="${BACKUP_DIR}/basebackups"
WAL_DIR="${BACKUP_DIR}/wal"
POSTGRES_USER="${POSTGRES_USER:-innovare}"
COMPOSE_PROJECT="${COMPOSE_PROJECT:-innovare-storage}"
TIMESTAMP="$(date +%Y%m%d_%H%M%S)"
COORDINATOR="${COMPOSE_PROJECT}-postgres-1"

# --- Parse arguments ---
while [[ $# -gt 0 ]]; do
    case "$1" in
        --backup-dir) BACKUP_DIR="$2"; BASEBACKUP_DIR="${BACKUP_DIR}/basebackups"; WAL_DIR="${BACKUP_DIR}/wal"; shift 2 ;;
        *) echo "Unknown argument: $1"; exit 1 ;;
    esac
done

# --- Ensure directories exist ---
mkdir -p "${BASEBACKUP_DIR}" "${WAL_DIR}"

BACKUP_NAME="basebackup_${TIMESTAMP}"
BACKUP_PATH="${BASEBACKUP_DIR}/${BACKUP_NAME}"

echo "=== Innovare Storage Base Backup ==="
echo "Timestamp: ${TIMESTAMP}"
echo "Backup path: ${BACKUP_PATH}"
echo ""

# --- Run pg_basebackup inside the container ---
echo "[1/3] Running pg_basebackup..."
docker exec "${COORDINATOR}" sh -c "rm -rf /tmp/basebackup && mkdir -p /tmp/basebackup"
docker exec "${COORDINATOR}" pg_basebackup \
    -U "${POSTGRES_USER}" \
    -D /tmp/basebackup \
    --format=tar \
    --gzip \
    --checkpoint=fast \
    --label="${BACKUP_NAME}" \
    --wal-method=none

# --- Copy out of container ---
echo "[2/3] Copying backup from container..."
mkdir -p "${BACKUP_PATH}"
docker cp "${COORDINATOR}:/tmp/basebackup/base.tar.gz" "${BACKUP_PATH}/base.tar.gz"
docker exec "${COORDINATOR}" rm -rf /tmp/basebackup

FINAL_SIZE="$(du -h "${BACKUP_PATH}/base.tar.gz" | cut -f1)"
echo "  Base backup size: ${FINAL_SIZE}"

# --- Clean old base backups and WAL files ---
echo "[3/3] Cleaning old backups..."

# Keep only last 2 base backups
BASEBACKUP_COUNT="$(find "${BASEBACKUP_DIR}" -maxdepth 1 -type d -name "basebackup_*" | wc -l)"
if [[ ${BASEBACKUP_COUNT} -gt 2 ]]; then
    find "${BASEBACKUP_DIR}" -maxdepth 1 -type d -name "basebackup_*" \
        | sort | head -n -2 \
        | while read -r old_dir; do
            echo "  Removing old base backup: $(basename "${old_dir}")"
            rm -rf "${old_dir}"
        done
fi

# Delete WAL files older than 7 days
find "${WAL_DIR}" -maxdepth 1 -type f -name "0000*" -mtime +7 -delete 2>/dev/null || true
WAL_COUNT="$(find "${WAL_DIR}" -maxdepth 1 -type f -name "0000*" 2>/dev/null | wc -l)"
echo "  WAL files remaining: ${WAL_COUNT}"

echo ""
echo "=== Base Backup Complete ==="
echo "File: ${BACKUP_PATH}/base.tar.gz"
echo "Size: ${FINAL_SIZE}"
echo "Timestamp: ${TIMESTAMP}"
