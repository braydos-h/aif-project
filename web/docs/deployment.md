# Production deployment

Two-user invite-only hosting on a Linux host with Caddy in front of the
Rust backend. Standalone open mode binds **0.0.0.0:8080** directly;
proxy-only production may bind **127.0.0.1** instead so only ports
80/443 are public. Database/operator administration never touches the public network.

## Prerequisites

- A Linux host (any small VPS), a domain pointing at it, ports 80+443 open.
- Build host with Rust 1.89+ (pinned by `rust-toolchain.toml`).
- `sqlite3`, `openssl`, Caddy on the target.

## 1. Build the release binary

```sh
cargo build --release --manifest-path backend/Cargo.toml
```

## 2. Install the service (as root on the target)

```sh
sudo sh deploy/install.sh
```

This creates the `aif` system user, `/opt/aif`, `/var/lib/aif` (mode 700),
`/etc/aif/aif.env` (mode 600, from `.env.example`), installs the systemd
unit and the Caddyfile skeleton, and enables the service.

## 3. Configure production (`/etc/aif/aif.env`)

Required values (startup refuses to boot in production without them):

```sh
AIF_PRODUCTION=1
AIF_REQUIRE_AUTH=1
AIF_PUBLIC_ORIGIN=https://cows.example.com
AIF_DATA_DIR=/var/lib/aif
AIF_OPERATOR_EMAIL=operator@example.com
AIF_AI_BACKEND=ollama
AIF_OLLAMA_URL=https://ollama.com/api/generate
AIF_AI_MODEL=gemma4:31b-cloud
OLLAMA_API_KEY=<paste the provider key>
AIF_TRUSTED_PROXIES=127.0.0.1
```

Optional tuning: `AIF_DAILY_LIMIT` (default 200), `AIF_MAX_INFERENCE`
(default 4), `AIF_SESSION_DAYS` (30), `AIF_INVITE_DAYS` (7),
`AIF_INFERENCE_PAUSED=1` to start paused.

Also edit `/etc/caddy/Caddyfile`: set the real domain and the alert
email. Enable the commented HSTS line **only after** HTTPS validates.

## 4. Start and invite

```sh
sudo systemctl start aif-backend
sudo systemctl status aif-backend
```

On first boot with an empty database, the service prints a one-time
operator invite link to the journal (`journalctl -u aif-backend`). Relay
it privately, or mint invites any time with:

```sh
sudo -u aif AIF_DATA_DIR=/var/lib/aif /opt/aif/aif-backend \
  --create-invite user@example.com --role user
sudo -u aif AIF_DATA_DIR=/var/lib/aif /opt/aif/aif-backend --list-invites
```

Then: `sudo systemctl reload caddy`, open `https://<domain>` on phone
mobile data, accept the invite, verify login → estimate → history →
logout → recovery. Keep the Windows/local launchers (`start_gui.bat`,
`start.sh`) untouched for offline use.

## Updates and rollback

```sh
# on the build host
cargo build --release --manifest-path backend/Cargo.toml
# on the target (backs up the DB first, auto-rolls-back on failed health)
sudo sh deploy/update.sh /path/to/new/aif-backend
sudo sh deploy/rollback.sh   # manual rollback to the previous binary
```

Migration order is backup → stop → swap → start → verify. Migrations
only ever add tables/columns/indexes, so the previous binary keeps
working against current data (forward repair is also possible).

## Ports and firewall

Public: 80/tcp + 443/tcp (proxy mode) or 8080/tcp (standalone open mode
binding 0.0.0.0). The SQLite database and operator CLI stay
loopback/localhost-only. `ufw allow 80,443/tcp` (or the cloud
firewall equivalent) and deny the rest; add `8080/tcp` only for direct
0.0.0.0 access.
