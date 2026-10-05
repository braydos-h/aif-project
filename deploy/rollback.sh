#!/bin/sh
# Roll back to the previous binary (saved by install.sh/update.sh).
# Run as root. Database migrations only ever add tables/columns, so the
# previous binary keeps working against current data.
set -eu

APP_DIR="/opt/aif"

if [ "$(id -u)" -ne 0 ]; then
  echo "Run as root: sudo sh deploy/rollback.sh" >&2
  exit 2
fi

if [ ! -f "$APP_DIR/aif-backend.prev" ]; then
  echo "No previous binary at $APP_DIR/aif-backend.prev" >&2
  exit 1
fi

cp "$APP_DIR/aif-backend" "$APP_DIR/aif-backend.failed"
cp "$APP_DIR/aif-backend.prev" "$APP_DIR/aif-backend"
systemctl restart aif-backend.service
sleep 3
curl -sf http://127.0.0.1:8080/health >/dev/null && echo "Rolled back and healthy."
