# Privacy and account controls

Two invited users. No public registration, no tracking, no analytics.

## What is stored

| Data | Where | Notes |
| --- | --- | --- |
| Email, display name, Argon2id password hash, role | SQLite `users` | Never passwords in clear; hashes never leave the server. |
| Invite/recovery metadata | SQLite `invites`, `recovery_tokens` | Token **hashes** only; raw tokens live in the operator's private relay and your cookie. |
| Sessions | SQLite `sessions` | Random token hashes + CSRF tokens; HttpOnly cookies; expiry + revocation. |
| Animals | SQLite `animals` | Owned by one account; never shared between the two users. |
| Estimates | SQLite `estimates` | Value, range, source/method, model/provider, estimator + prompt versions, tape inputs, animal hints, timestamps, placeholder flag. **No photos, raw model dumps, or API keys.** |
| Background job requests | SQLite `jobs.payload` | A queued photo is temporarily stored as a JSON request payload so it can survive reconnects/restarts. The payload is cleared when the job becomes success, failed, cancelled, or expired. Queued work is deliberately omitted from sanitized backup snapshots. |
| Retained photos (only with `AIF_RETAIN_PHOTOS=1` + per-estimate opt-in) | `<data_dir>/photos/<opaque-id>` + `photos` rows | Metadata-stripped bytes, owner + optional estimate link, expiry, size/MIME. Bytes are excluded from backups; metadata rows may remain in the SQLite snapshot. Never shared. |
| Usage counters, audit log | SQLite | Counts per day; audit entries exclude secrets/tokens/prompts/images. |

## What is NOT stored by default

- Synchronous uploaded photographs are decoded, validated, sent to the AI
  provider for the estimate, then dropped from server memory; they are not
  written to disk unless photo retention is explicitly enabled.
- Background uploads are an exception while work is pending: their encoded
  request payloads are stored in SQLite so jobs can be resumed after a client
  disconnect or server restart. Terminal jobs clear the payload. The result
  row stores estimate metadata only.

## Optional photo retention (`AIF_RETAIN_PHOTOS=1`)

Off by default. When the operator enables it, a logged-in estimate with
`retain_photo: true` stores the processed photo:

- opaque 32-hex server id under `<data_dir>/photos/` (0600), never a
  user-derived path; download (`GET /api/photos/{id}`) is owner-only
  with `Cache-Control: private` and no CORS sharing;
- JPEG APP1/Exif and PNG eXIf segments stripped (no location metadata);
- per-user quota (`AIF_PHOTO_QUOTA_MB`, default 50 MiB) and lifetime
  (`AIF_PHOTO_TTL_DAYS`, default 30 days); expiry and orphans swept at
  startup and periodically;
- deleting a history row deletes its linked photos; deleting an account
  deletes all of its photos (rows cascade, files removed explicitly);
- retained photo **files/bytes are excluded from database backups**; SQLite
  photo metadata rows remain in snapshots. The photo dir is covered only if
  the operator backs it up separately (document the same 14-day rotation if
  you do).
- API keys, passwords, session tokens, invite/reset tokens (hashes only).
- Base64 images in browser storage (the WebUI uses no `localStorage`).

## AI / provider processing

Photo estimates transmit your image + prompt to the configured provider
(Ollama Cloud by default) and store only the returned numbers. The
footer of every result and export carries the dosing disclaimer:
estimates are not scale, vet, or dosing advice. Deterministic
`local_fallback` results are labeled **offline placeholders**, never AI
measurements.

## Logs and retention

- Server logs: request ids, errors, cache/connection counters. No photos,
  credentials, prompts, or tokens.
- Active storage: your rows live until you delete them (history rows,
  animals, retained photos) or delete your account (everything, sessions
  revoked, in one transaction). Retained photos additionally expire
  automatically after `AIF_PHOTO_TTL_DAYS`.
- Backups: 14 daily encrypted off-server copies. The backup script marks
  queued/active jobs expired and clears every job payload from the snapshot,
  so queued photos are not recoverable from a backup. Terminal estimate
  metadata remains in the snapshot; deleted data ages out as backups rotate
  (worst case 14 days). Retained photo files are excluded.
- Support contact: the operator (see your invite email). Region-specific
  assessment: this is a private two-user tool with no public offering;
  confirm hosting-region obligations with the operator before launch.

## Your controls (Account view, all require login)

- **Export history CSV** / **Export all my data (JSON)**: everything the
  account owns. CSV text fields are formula-neutralized and quoted;
  numeric columns stay numeric.
- **Change password** (needs the current password; logs you out
  everywhere). **Delete account** (needs password + checkbox): revokes
  every session and deletes all owned rows immediately.
- Batch estimates finish server-side once sent; results already saved
  stay saved — delete them from history if unwanted. Cancelling a batch
  clears queued payloads, while active inference may finish and remains
  visible to its owner. Logging out does not cancel in-flight work, and
  never exposes its results to the other account.
- Estimate limits and dosing warnings stay visible in results, history,
  and exports.
