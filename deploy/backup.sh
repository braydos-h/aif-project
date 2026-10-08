#!/bin/sh
# Encrypted off-server backup of the SQLite database.
# Usage: sh deploy/backup.sh [DATA_DIR] [BACKUP_DIR]
# Requires: sqlite3, openssl. The encryption passphrase MUST come from
# AIF_BACKUP_PASSPHRASE (never a committed file).
#
# Retention: keeps the last 14 daily backups in BACKUP_DIR; the operator
# copies them off-server (see docs/operations.md). Background photo payloads
# are temporary in SQLite and are removed from the snapshot before encryption.
set -eu

DATA_DIR="${1:-/var/lib/aif}"
BACKUP_DIR="${2:-/var/backups/aif}"

if [ -z "${AIF_BACKUP_PASSPHRASE:-}" ]; then
  echo "Set AIF_BACKUP_PASSPHRASE in the environment (not in a file)." >&2
  exit 2
fi
command -v sqlite3 >/dev/null 2>&1 || { echo "sqlite3 is required" >&2; exit 2; }

DB="$DATA_DIR/aif.db"
if [ ! -f "$DB" ]; then
  echo "No database at $DB — nothing to back up." >&2
  exit 1
fi

mkdir -p "$BACKUP_DIR"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT INT TERM

# Online-safe snapshot: SQLite backup API, not a raw file copy.
sqlite3 "$DB" ".backup '$TMP/aif.db'"
# A restore contains estimate history, not queued photos. Mark unfinished jobs
# expired and clear all job payloads from the snapshot before it is encrypted.
sqlite3 "$TMP/aif.db" "UPDATE jobs SET status = 'expired', updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'), payload = '{}', error_code = 'backup_restore', error_message = 'Queued work was omitted from this backup' WHERE status IN ('queued', 'active'); UPDATE jobs SET payload = '{}';"
printf '%s' "$STAMP" > "$TMP/stamp.txt"
tar -czf "$TMP/aif-$STAMP.tar.gz" -C "$TMP" aif.db stamp.txt
openssl enc -aes-256-cbc -salt -pbkdf2 -in "$TMP/aif-$STAMP.tar.gz" \
  -out "$BACKUP_DIR/aif-$STAMP.tar.gz.enc" -pass env:AIF_BACKUP_PASSPHRASE
chmod 600 "$BACKUP_DIR/aif-$STAMP.tar.gz.enc"

# Retention: keep the newest 14, prune older ones.
# shellcheck disable=SC2012
ls -1t "$BACKUP_DIR"/aif-*.tar.gz.enc 2>/dev/null | tail -n +15 | xargs -r rm -f
echo "Backup complete: $BACKUP_DIR/aif-$STAMP.tar.gz.enc"
echo "Copy it OFF-SERVER now (scp/rsync to encrypted storage); local copies do not survive host loss."
