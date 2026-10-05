#!/bin/sh
# Update the running service to a new release binary, keeping the previous
# binary for instant rollback. Run as root.
# Usage: sudo sh deploy/update.sh /path/to/new/aif-backend
set -eu

APP_DIR="/opt/aif"

if [ "$(id -u)" -ne 0 ]; then
  echo "Run as root: sudo sh deploy/update.sh /path/to/new/aif-backend" >&2
  exit 2
fi

NEW_BIN="${1:-}"
if [ -z "$NEW_BIN" ] || [ ! -x "$NEW_BIN" ]; then
  echo "Usage: $0 /path/to/new/aif-backend" >&2
  exit 2
fi

# Back up the database before touching the service (migration-safe order:
# backup -> stop -> swap -> start -> verify).
if [ -f /etc/aif/aif.env ]; then
  # shellcheck disable=SC1091
  DATA_DIR="$(grep -E '^AIF_DATA_DIR=' /etc/aif/aif.env | cut -d= -f2-)"
fi
DATA_DIR="${DATA_DIR:-/var/lib/aif}"
"$(dirname "$0")/backup.sh" "${DATA_DIR}"

if [ -f "$APP_DIR/aif-backend" ]; then
  cp "$APP_DIR/aif-backend" "$APP_DIR/aif-backend.prev"
fi
cp "$NEW_BIN" "$APP_DIR/aif-backend.new"
chmod 755 "$APP_DIR/aif-backend.new"
mv "$APP_DIR/aif-backend.new" "$APP_DIR/aif-backend"

systemctl restart aif-backend.service
sleep 3
if systemctl is-active --quiet aif-backend.service && curl -sf http://127.0.0.1:8080/health >/dev/null; then
  echo "Update healthy. Roll back with: sudo sh deploy/rollback.sh"
else
  echo "UPDATE FAILED health check — rolling back automatically" >&2
  sh "$(dirname "$0")/rollback.sh"
  exit 1
fi
