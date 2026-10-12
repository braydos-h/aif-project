#!/bin/sh
# Install the Cow Weight Estimator as a non-root systemd service.
# Run as root on the target host. Idempotent: safe to rerun for updates
# of scripts/config (use update.sh to swap the binary).
set -eu

APP_USER="aif"
APP_DIR="/opt/aif"
DATA_DIR="/var/lib/aif"
ENV_DIR="/etc/aif"
REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"

if [ "$(id -u)" -ne 0 ]; then
  echo "Run as root: sudo sh deploy/install.sh" >&2
  exit 2
fi

command -v sqlite3 >/dev/null 2>&1 || { echo "sqlite3 is required (apt install sqlite3)" >&2; exit 2; }

id "$APP_USER" >/dev/null 2>&1 || useradd --system --no-create-home --shell /usr/sbin/nologin "$APP_USER"

mkdir -p "$APP_DIR" "$DATA_DIR" "$ENV_DIR" /var/log/caddy
chown "$APP_USER:$APP_USER" "$DATA_DIR"
chmod 700 "$DATA_DIR"
chmod 755 "$APP_DIR"

cp "$REPO_DIR/deploy/aif-backend.service" /etc/systemd/system/aif-backend.service
cp "$REPO_DIR/deploy/Caddyfile" /etc/caddy/Caddyfile 2>/dev/null || cp "$REPO_DIR/deploy/Caddyfile" "$APP_DIR/Caddyfile.deployed"
cp "$REPO_DIR/deploy/logrotate-aif" /etc/logrotate.d/aif 2>/dev/null || true

if [ ! -f "$ENV_DIR/aif.env" ]; then
  sed -e "s#^AIF_DATA_DIR=.*#AIF_DATA_DIR=$DATA_DIR#" "$REPO_DIR/.env.example" > "$ENV_DIR/aif.env"
  chmod 600 "$ENV_DIR/aif.env"
  chown root:root "$ENV_DIR/aif.env"
  echo "Created $ENV_DIR/aif.env from .env.example — EDIT IT before starting:"
  echo "  AIF_PRODUCTION=1 AIF_REQUIRE_AUTH=1 AIF_PUBLIC_ORIGIN=https://<domain>"
  echo "  OLLAMA_API_KEY=<key> AIF_OPERATOR_EMAIL=<operator email>"
fi

if [ -x "$REPO_DIR/backend/target/release/aif-backend" ]; then
  cp "$REPO_DIR/backend/target/release/aif-backend" "$APP_DIR/aif-backend.new"
  mv "$APP_DIR/aif-backend.new" "$APP_DIR/aif-backend"
  chmod 755 "$APP_DIR/aif-backend"
else
  echo "No release binary at backend/target/release/aif-backend." >&2
  echo "Build on a build host: cargo build --release --manifest-path backend/Cargo.toml" >&2
  echo "then copy backend/target/release/aif-backend to $APP_DIR/aif-backend" >&2
fi

systemctl daemon-reload
systemctl enable aif-backend.service
echo "Installed. Next:"
echo "  1. Edit $ENV_DIR/aif.env (production values, see docs/deployment.md)"
echo "  2. systemctl start aif-backend && systemctl status aif-backend"
echo "  3. Mint the operator invite: sudo -u $APP_USER AIF_DATA_DIR=$DATA_DIR /opt/aif/aif-backend --create-invite operator@example.com --role operator"
