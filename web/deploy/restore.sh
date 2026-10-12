#!/bin/sh
# Restore the database from an encrypted backup and verify it.
# Usage: sudo sh deploy/restore.sh BACKUP_FILE [DATA_DIR]
# The service is stopped first; the live database is preserved as
# aif.db.pre-restore-<stamp> for forward repair.
set -eu

BACKUP_FILE="${1:-}"
DATA_DIR="${2:-/var/lib/aif}"

if [ "$(id -u)" -ne 0 ]; then
  echo "Run as root: sudo sh deploy/restore.sh BACKUP_FILE [DATA_DIR]" >&2
  exit 2
fi
if [ -z "$BACKUP_FILE" ] || [ ! -f "$BACKUP_FILE" ]; then
  echo "Usage: $0 BACKUP_FILE [DATA_DIR]" >&2
  exit 2
fi
if [ -z "${AIF_BACKUP_PASSPHRASE:-}" ]; then
  echo "Set AIF_BACKUP_PASSPHRASE in the environment (not in a file)." >&2
  exit 2
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT INT TERM

openssl enc -d -aes-256-cbc -pbkdf2 -in "$BACKUP_FILE" \
  -out "$TMP/restore.tar.gz" -pass env:AIF_BACKUP_PASSPHRASE
tar -xzf "$TMP/restore.tar.gz" -C "$TMP"

systemctl stop aif-backend.service || true
if [ -f "$DATA_DIR/aif.db" ]; then
  STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
  cp "$DATA_DIR/aif.db" "$DATA_DIR/aif.db.pre-restore-$STAMP"
  echo "Live database preserved at $DATA_DIR/aif.db.pre-restore-$STAMP"
fi
cp "$TMP/aif.db" "$DATA_DIR/aif.db"
chown aif:aif "$DATA_DIR/aif.db"
chmod 600 "$DATA_DIR/aif.db"
systemctl start aif-backend.service
sleep 3
curl -sf http://127.0.0.1:8080/health >/dev/null && echo "Restore healthy."
echo "Verify now: log in as operator, check /api/operator/status user count and recent history."
