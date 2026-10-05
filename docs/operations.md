# Operations, monitoring, and backups

## Operator controls

- WebUI **Account → Operator** (role `operator`): invites (create/revoke),
  accounts, per-day usage, inference pause switch, redacted audit log.
- CLI (same database, `AIF_DATA_DIR` must match the service):

```sh
sudo -u aif AIF_DATA_DIR=/var/lib/aif /opt/aif/aif-backend --list-users
sudo -u aif AIF_DATA_DIR=/var/lib/aif /opt/aif/aif-backend --create-invite user@example.com
sudo -u aif AIF_DATA_DIR=/var/lib/aif /opt/aif/aif-backend --revoke-invite <id>
sudo -u aif AIF_DATA_DIR=/var/lib/aif /opt/aif/aif-backend --delete-user user@example.com
sudo -u aif AIF_DATA_DIR=/var/lib/aif /opt/aif/aif-backend --create-recovery user@example.com
```

Security-sensitive actions (login, invite create/accept/revoke, recovery,
password change, pause, exports, deletions) are appended to the audit log
**without** secrets, tokens, prompts, or images.

## Monitoring

| Signal | How |
| --- | --- |
| Uptime | External check on `https://<domain>/health` (expects `{"status":"ok"}`); systemd `Restart=on-failure` + `journalctl -u aif-backend`. |
| Provider failures | `journalctl` for `estimation failed`; operator `/api/operator/usage` and audit; `provider_errors` alert in `/api/operator/status` at 5+/hour. |
| Usage spikes | `usage_high` alert in `/api/operator/status` at 80% of `AIF_DAILY_LIMIT`; shown in the operator panel. |
| Disk capacity | Alert on `/var/lib/aif` and `/var/backups/aif` (`disk_high` alert in status past `AIF_DISK_ALERT_MB`; DB rows plus any retained photos under `<data_dir>/photos`). |
| Certificate expiry | Caddy renews automatically; the `email` in the Caddyfile gets expiry alerts. Verify with `curl -vI https://<domain>`. |
| Provider spending | `AIF_DAILY_LIMIT` per user + `AIF_MAX_INFERENCE` concurrency + bounded single retry; pause inference from the operator panel (`POST /api/operator/pause`). Unexpected traffic answers 429/503, never unbounded inference. |

## Backups

```sh
# nightly cron (passphrase from a secret manager, never a file):
AIF_BACKUP_PASSPHRASE=... sh /opt/aif/deploy/backup.sh /var/lib/aif /var/backups/aif
```

Each run takes an online-safe SQLite snapshot, packs it with a timestamp,
encrypts with AES-256-CBC (PBKDF2), keeps the newest 14, and prints a
reminder to copy the file **off-server** (scp/rsync to encrypted
storage). Local copies do not survive host loss. Photo backups are
unneeded: the service stores no photos.

Retention: 14 daily encrypted copies off-server (database only —
retained photos in `<data_dir>/photos` are excluded; back that dir up
separately under the same rotation if retention is enabled); account
deletion removes rows from active storage immediately, while
already-shipped backup copies age out within the 14-day window (see
`docs/privacy.md`).

## Restore drill (required before launch, repeat yearly)

```sh
sudo AIF_BACKUP_PASSPHRASE=... sh deploy/restore.sh /path/to/aif-<stamp>.tar.gz.enc /var/lib/aif
```

Evidence to record: stamp restored, `curl /health`, operator login,
`/api/operator/status` user count, one history row readable, then file
the drill date in `todo/15-operations-backups.md`. The live DB is
preserved as `aif.db.pre-restore-<stamp>` for forward repair.

## Incident playbooks

- **Outage**: `systemctl status aif-backend`, `journalctl -u aif-backend -n 100`,
  `curl http://127.0.0.1:8080/health`; restart, then roll back if the
  release is suspect (`deploy/rollback.sh`). Provider outage shows as
  explicit 502s (never silent placeholders); pause inference if spend is
  at risk.
- **Credential compromise**: rotate `OLLAMA_API_KEY` at the provider,
  update `/etc/aif/aif.env`, `systemctl restart`; force user password
  resets via `--create-recovery`; review the audit log.
- **Key rotation**: provider keys at
  `https://ollama.com/settings/keys`; session signing is random per
  session (no static secret); backup passphrase rotated by re-encrypting
  the newest backup.
- **Dependency/model updates**: review changelogs, rerun
  `cargo test` + `aif-eval` on the reference set, deploy via
  `deploy/update.sh`, keep the previous binary for rollback.
- **Domain/host/provider expiry or payment failure**: renew, then
  re-verify external phone access; Caddy re-issues certificates
  automatically once DNS resolves again.
- **Service closure**: both users export (`Account → Export`, CSV +
  JSON), operator shuts off invites (`--revoke-invite` each active one),
  stops the service, revokes the provider key, deletes data per
  `docs/privacy.md` retention, then decommissions DNS/hosting.
