#!/usr/bin/env bash
# Cron wrapper for backup.sh with logging and rotation
#
# Usage: ./backup-cron.sh
# Environment: PROFILE, BACKUP_DIR, BACKUP_RETENTION_DAYS, WEBHOOK_URL (optional)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BACKUP_DIR="${BACKUP_DIR:-/backups}"
BACKUP_RETENTION_DAYS="${BACKUP_RETENTION_DAYS:-30}"
LOG_DIR="${BACKUP_DIR}/logs"
LOG_FILE="${LOG_DIR}/backup_$(date +%Y%m%d_%H%M%S).log"
WEBHOOK_URL="${WEBHOOK_URL:-}"

mkdir -p "${LOG_DIR}" "${BACKUP_DIR}/wal" "${BACKUP_DIR}/basebackups"

# --- Logging ---
log() {
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*" | tee -a "${LOG_FILE}"
}

# --- Notify on error (optional webhook) ---
notify_error() {
    local message="$1"
    log "ERROR: ${message}"
    if [[ -n "${WEBHOOK_URL}" ]]; then
        local payload
        payload="$(jq -n --arg text "Innovare Storage backup FAILED: ${message}" '{text: $text}')"
        curl -sf -X POST "${WEBHOOK_URL}" \
            -H "Content-Type: application/json" \
            -d "${payload}" \
            >> "${LOG_FILE}" 2>&1 || true
    fi
}

log "=== Starting scheduled backup ==="

# --- Run backup ---
if "${SCRIPT_DIR}/backup.sh" >> "${LOG_FILE}" 2>&1; then
    log "Backup completed successfully"
else
    EXIT_CODE=$?
    notify_error "backup.sh exited with code ${EXIT_CODE}"
    exit ${EXIT_CODE}
fi

# --- Rotate old backups ---
log "Rotating backups older than ${BACKUP_RETENTION_DAYS} days..."
DELETED_COUNT=0
while IFS= read -r old_backup; do
    log "  Deleting: $(basename "${old_backup}")"
    rm -f "${old_backup}"
    DELETED_COUNT=$((DELETED_COUNT + 1))
done < <(find "${BACKUP_DIR}" -maxdepth 1 -name "backup_*.tar.gz" -mtime "+${BACKUP_RETENTION_DAYS}" -type f 2>/dev/null)
log "Deleted ${DELETED_COUNT} old backup(s)"

# --- Rotate old logs (keep 90 days) ---
find "${LOG_DIR}" -maxdepth 1 -name "backup_*.log" -mtime +90 -type f -delete 2>/dev/null || true

# --- WAL file rotation (keep last 3 days) ---
WAL_DIR="${BACKUP_DIR}/wal"
WAL_DELETED=0
while IFS= read -r old_wal; do
    rm -f "${old_wal}"
    WAL_DELETED=$((WAL_DELETED + 1))
done < <(find "${WAL_DIR}" -maxdepth 1 -type f -name "0000*" -mtime +3 2>/dev/null)
log "Deleted ${WAL_DELETED} old WAL file(s)"

# --- Weekly base backup (Sundays) for PITR ---
if [[ "$(date +%u)" -eq 7 ]]; then
    log "Running weekly base backup..."
    if "${SCRIPT_DIR}/basebackup.sh" >> "${LOG_FILE}" 2>&1; then
        log "Base backup completed successfully"
    else
        notify_error "basebackup.sh exited with code $?"
    fi
fi

# --- Check disk space ---
if [ -x "${SCRIPT_DIR}/check-disk.sh" ]; then
    "${SCRIPT_DIR}/check-disk.sh" >> "${LOG_FILE}" 2>&1 || true
fi

log "=== Scheduled backup finished ==="
