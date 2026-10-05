# Privacy and account controls

Two invited users. No public registration, no tracking, no analytics.

## What is stored

| Data | Where | Notes |
| --- | --- | --- |
| Email, display name, Argon2id password hash, role | SQLite `users` | Never passwords in clear; hashes never leave the server. |
| Invite/recovery metadata | SQLite `invites`, `recovery_tokens` | Token **hashes** only; raw tokens live in the operator's private relay and your cookie. |
| Sessions | SQLite `sessions` | Random token hashes + CSRF tokens; HttpOnly cookies; expiry + revocation. |
| Animals | SQLite `animals` | Owned by one account; never shared between the two users. |
| Estimates | SQLite `estimates` | Value, range, source/method, model/provider, estimator + prompt versions, tape inputs, animal hints, timestamps, placeholder flag. **No photos, no raw model dumps, no API keys.** |
| Usage counters, audit log | SQLite | Counts per day; audit entries exclude secrets/tokens/prompts/images. |

## What is NOT stored

- Uploaded photographs (transient-only: decoded, validated, sent to the
  AI provider for the estimate, then dropped — never written to disk).
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
  animals) or delete your account (everything, sessions revoked, in one
  transaction).
- Backups: 14 daily encrypted off-server copies; deleted data ages out
  as backups rotate (worst case 14 days). There is no longer-lived copy
  by design.
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
  stay saved — delete them from history if unwanted. Logging out does
  not cancel an in-flight batch, and never exposes its results to the
  other account.
- Estimate limits and dosing warnings stay visible in results, history,
  and exports.
