#!/usr/bin/env bash
# Check backup volume free space and alert via Telegram if low
#
# Usage: ./check-disk.sh
# Environment: TELEGRAM_BOT_TOKEN, TELEGRAM_CHAT_ID, BACKUP_DIR, DISK_THRESHOLD
set -euo pipefail

BACKUP_DIR="${BACKUP_DIR:-/backups}"
DISK_THRESHOLD="${DISK_THRESHOLD:-10}"  # alert if free space <= this %
TELEGRAM_BOT_TOKEN="${TELEGRAM_BOT_TOKEN:-}"
TELEGRAM_CHAT_ID="${TELEGRAM_CHAT_ID:-}"

# Get usage percentage (e.g. "42%")
USAGE="$(df "${BACKUP_DIR}" | awk 'NR==2 {print $5}' | tr -d '%')"
FREE=$((100 - USAGE))

echo "Backup volume: ${USAGE}% used, ${FREE}% free (threshold: ${DISK_THRESHOLD}%)"
df -h "${BACKUP_DIR}" | tail -1

if [[ ${FREE} -le ${DISK_THRESHOLD} ]]; then
    echo "WARNING: Low disk space on backup volume!"

    DISK_INFO="$(df -h "${BACKUP_DIR}" | awk 'NR==2 {printf "Size: %s, Used: %s, Free: %s", $2, $3, $4}')"
    WAL_SIZE="$(du -sh "${BACKUP_DIR}/wal" 2>/dev/null | cut -f1 || echo "N/A")"
    BACKUP_SIZE="$(du -sh "${BACKUP_DIR}" 2>/dev/null | cut -f1 || echo "N/A")"

    MESSAGE="⚠️ *Innovare Storage: Low Disk Space*

Backup volume: *${FREE}% free* (threshold: ${DISK_THRESHOLD}%)
${DISK_INFO}
WAL files: ${WAL_SIZE}
Total backups: ${BACKUP_SIZE}

Host: $(hostname)"

    if [[ -n "${TELEGRAM_BOT_TOKEN}" && -n "${TELEGRAM_CHAT_ID}" ]]; then
        curl -sf -X POST "https://api.telegram.org/bot${TELEGRAM_BOT_TOKEN}/sendMessage" \
            -d chat_id="${TELEGRAM_CHAT_ID}" \
            -d parse_mode=Markdown \
            -d text="${MESSAGE}" > /dev/null
        echo "Telegram alert sent."
    else
        echo "No Telegram credentials configured. Alert not sent."
        echo "${MESSAGE}"
    fi
else
    echo "OK: Disk space within limits."
fi
