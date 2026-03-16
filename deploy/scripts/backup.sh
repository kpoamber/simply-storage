#!/usr/bin/env bash
# PostgreSQL/Citus backup script for Innovare Storage
# Supports standalone postgres (small profile) and Citus (medium/large profiles)
#
# Usage: ./backup.sh [--profile small|medium|large] [--backup-dir /path]
# Environment: POSTGRES_USER, POSTGRES_PASSWORD, POSTGRES_DB, COMPOSE_PROJECT
set -euo pipefail

# --- Configuration ---
PROFILE="${PROFILE:-small}"
BACKUP_DIR="${BACKUP_DIR:-/backups}"
POSTGRES_USER="${POSTGRES_USER:-innovare}"
POSTGRES_DB="${POSTGRES_DB:-innovare_storage}"
COMPOSE_PROJECT="${COMPOSE_PROJECT:-innovare-storage}"
TIMESTAMP="$(date +%Y%m%d_%H%M%S)"

# --- Parse arguments ---
while [[ $# -gt 0 ]]; do
    case "$1" in
        --profile) PROFILE="$2"; shift 2 ;;
        --backup-dir) BACKUP_DIR="$2"; shift 2 ;;
        *) echo "Unknown argument: $1"; exit 1 ;;
    esac
done

BACKUP_NAME="backup_${PROFILE}_${TIMESTAMP}"
BACKUP_PATH="${BACKUP_DIR}/${BACKUP_NAME}"

# --- Ensure backup directory exists ---
mkdir -p "${BACKUP_PATH}"

echo "=== Innovare Storage Backup ==="
echo "Profile: ${PROFILE}"
echo "Timestamp: ${TIMESTAMP}"
echo "Backup path: ${BACKUP_PATH}"
echo ""

# --- Helper: run pg_dump inside a docker container ---
dump_container() {
    local container="$1"
    local output_file="$2"
    echo "  Dumping ${container} -> ${output_file}..."
    docker exec "${container}" pg_dump \
        -U "${POSTGRES_USER}" \
        -d "${POSTGRES_DB}" \
        --no-owner \
        --no-privileges \
        --format=custom \
        > "${output_file}"
    echo "  Done: $(du -h "${output_file}" | cut -f1)"
}

# --- Determine containers based on profile ---
COORDINATOR="${COMPOSE_PROJECT}-postgres-1"

case "${PROFILE}" in
    small)
        echo "[1/2] Backing up standalone PostgreSQL..."
        dump_container "${COORDINATOR}" "${BACKUP_PATH}/coordinator.dump"
        echo ""
        ;;
    medium)
        echo "[1/4] Backing up Citus coordinator..."
        dump_container "${COORDINATOR}" "${BACKUP_PATH}/coordinator.dump"
        echo ""
        for i in 1 2; do
            WORKER="${COMPOSE_PROJECT}-postgres-worker-${i}-1"
            echo "[$(( i + 1 ))/4] Backing up worker ${i}..."
            dump_container "${WORKER}" "${BACKUP_PATH}/worker_${i}.dump"
            echo ""
        done
        ;;
    large)
        echo "[1/6] Backing up Citus coordinator..."
        dump_container "${COORDINATOR}" "${BACKUP_PATH}/coordinator.dump"
        echo ""
        for i in 1 2 3 4; do
            WORKER="${COMPOSE_PROJECT}-postgres-worker-${i}-1"
            echo "[$(( i + 1 ))/6] Backing up worker ${i}..."
            dump_container "${WORKER}" "${BACKUP_PATH}/worker_${i}.dump"
            echo ""
        done
        ;;
    *)
        echo "ERROR: Unknown profile '${PROFILE}'. Must be small, medium, or large."
        exit 1
        ;;
esac

# --- Compress backup ---
echo "Compressing backup..."
tar -czf "${BACKUP_PATH}.tar.gz" -C "${BACKUP_DIR}" "${BACKUP_NAME}"
rm -rf "${BACKUP_PATH}"

FINAL_SIZE="$(du -h "${BACKUP_PATH}.tar.gz" | cut -f1)"
echo ""
echo "=== Backup Complete ==="
echo "File: ${BACKUP_PATH}.tar.gz"
echo "Size: ${FINAL_SIZE}"
echo "Timestamp: ${TIMESTAMP}"
