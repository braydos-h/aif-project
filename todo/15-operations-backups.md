# 15. Operations, monitoring, and backups

**Depends on:** 10, 11, 12, 14

## Todo

- [x] Provide minimal protected operator controls for invitations, revocation, usage, and pausing inference.
- [x] Log security-sensitive actions and request IDs without photos, credentials, private prompts, or reset tokens.
- [x] Monitor uptime, provider failures, disk capacity, certificate expiry, and spending. (Guidance + endpoints in docs/operations.md; external uptime checker to be configured on the live host.)
- [x] Automate encrypted off-server database backups with defined retention. (deploy/backup.sh: online-safe snapshot, encrypted output, 14-copy retention; queued/active jobs are expired and every job payload is cleared in the snapshot. Opt-in retained photo files are excluded.)
- [ ] Perform a restore drill and document updates, rollback, outages, credential compromise, and operator recovery.

- [x] Document provider-key rotation, dependency/model update review, and expiry or payment failures for the domain, host, and provider account.
- [x] Keep a tested service closure procedure covering user exports, invitation shutdown, credential revocation, and deletion under the agreed retention policy.

## Completion check

Alerts reach the operator and a tested restore recovers the two-user service.

## Implementation status (2026-10-05)

Repo-side tooling and docs are complete. The restore drill (`sudo AIF_BACKUP_PASSPHRASE=... sh deploy/restore.sh <file>`) must still be EXECUTED against real backup infrastructure: restore to a scratch host, verify health + operator login + history, and file the dated evidence here. That box stays unchecked until the drill log exists.
