#!/usr/bin/env bash
# Weekly system health report sent via Telegram
#
# Usage: ./weekly-report.sh
# Environment: TELEGRAM_BOT_TOKEN, TELEGRAM_CHAT_ID, COMPOSE_PROJECT, POSTGRES_USER, POSTGRES_DB
set -euo pipefail

TELEGRAM_BOT_TOKEN="${TELEGRAM_BOT_TOKEN:-}"
TELEGRAM_CHAT_ID="${TELEGRAM_CHAT_ID:-}"
COMPOSE_PROJECT="${COMPOSE_PROJECT:-innovare-storage}"
POSTGRES_USER="${POSTGRES_USER:-innovare}"
POSTGRES_DB="${POSTGRES_DB:-innovare_storage}"
COORDINATOR="${COMPOSE_PROJECT}-postgres-1"

# --- Root disk ---
ROOT_DISK="$(df -h / | awk 'NR==2 {printf "%s / %s (%s used)", $3, $2, $5}')"

# --- Backup volume ---
BACKUP_DISK="$(df -h /backups | awk 'NR==2 {printf "%s / %s (%s used)", $3, $2, $5}')"
WAL_SIZE="$(du -sh /backups/wal 2>/dev/null | cut -f1 || echo "0")"
WAL_COUNT="$(find /backups/wal -maxdepth 1 -type f -name '0000*' 2>/dev/null | wc -l)"
DUMP_SIZE="$(du -sh /backups/backup_*.tar.gz 2>/dev/null | tail -1 | cut -f1 || echo "0")"
BASE_SIZE="$(du -sh /backups/basebackups 2>/dev/null | cut -f1 || echo "0")"

# --- Database size ---
DB_SIZE="$(docker exec "${COORDINATOR}" psql -U "${POSTGRES_USER}" -d "${POSTGRES_DB}" -tAc \
  "SELECT pg_size_pretty(pg_database_size('${POSTGRES_DB}'));" 2>/dev/null || echo "N/A")"

DB_TABLES="$(docker exec "${COORDINATOR}" psql -U "${POSTGRES_USER}" -d "${POSTGRES_DB}" -tAc \
  "SELECT string_agg(t, E'\n') FROM (
    SELECT format('%s: %s', relname, pg_size_pretty(pg_total_relation_size(c.oid))) AS t
    FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
    WHERE n.nspname = 'public' AND c.relkind = 'r'
    ORDER BY pg_total_relation_size(c.oid) DESC LIMIT 5
  ) sub;" 2>/dev/null || echo "N/A")"

# --- App storage ---
APP_DATA="$(docker exec "${COMPOSE_PROJECT}-app-1" du -sh /app/data 2>/dev/null | cut -f1 || echo "N/A")"

# --- Docker ---
DOCKER_DISK="$(docker system df --format '{{.Type}}: {{.Size}} (reclaimable: {{.Reclaimable}})' 2>/dev/null | tr '\n' '\n' || echo "N/A")"

# --- Containers ---
CONTAINERS="$(docker ps --format '{{.Names}}: {{.Status}}' --filter "label=com.docker.compose.project=${COMPOSE_PROJECT}" 2>/dev/null || echo "N/A")"

# --- Uptime ---
UPTIME="$(uptime -p 2>/dev/null || echo "N/A")"

MESSAGE="📊 <b>Weekly Report — Innovare Storage</b>

<b>Server:</b> $(hostname) | ${UPTIME}

<b>💾 Disks</b>
Root: ${ROOT_DISK}
Backup volume: ${BACKUP_DISK}

<b>🗄 Database</b>
Size: ${DB_SIZE}
Top tables:
<code>${DB_TABLES}</code>

<b>📦 Storage</b>
App data: ${APP_DATA}
WAL files: ${WAL_SIZE} (${WAL_COUNT} files)
Base backups: ${BASE_SIZE}
Latest dump: ${DUMP_SIZE}

<b>🐳 Docker</b>
<code>${DOCKER_DISK}</code>

<b>🟢 Containers</b>
<code>${CONTAINERS}</code>"

echo "${MESSAGE}"

if [[ -n "${TELEGRAM_BOT_TOKEN}" && -n "${TELEGRAM_CHAT_ID}" ]]; then
    curl -sf -X POST "https://api.telegram.org/bot${TELEGRAM_BOT_TOKEN}/sendMessage" \
        -d chat_id="${TELEGRAM_CHAT_ID}" \
        -d parse_mode=HTML \
        -d text="${MESSAGE}" > /dev/null
    echo ""
    echo "Report sent to Telegram."
else
    echo ""
    echo "No Telegram credentials. Report not sent."
fi
