# 3. Invite-only authentication and sessions

**Depends on:** 0, 2

## Todo

- [x] Let only the operator invite the two approved users; do not expose a public signup endpoint.
- [x] Use expiring, single-use, cryptographically random invites tied to intended recipients; define replacement/revocation without accidentally allowing a third active account.
- [x] Use an established authentication provider or maintained password-hashing/session libraries; do not invent cryptography.
- [x] Use secure HttpOnly session cookies, appropriate SameSite/CSRF protections, expiry, rotation, logout, and revocation.
- [x] Choose email recovery or a documented secure operator recovery procedure; support administrator MFA where available.
- [x] Prevent account enumeration and rate-limit login, invite acceptance, and recovery.

## Completion check

Uninvited users cannot create accounts; expired/reused/revoked invites fail; only two approved active users can authenticate.

## Implementation status (2026-10-05)

All repo-side items are implemented and covered by integration tests (26 auth/isolation tests). Email delivery was replaced by the documented secure operator relay (docs/recovery.md). App-level administrator MFA is out of scope: admin access is host SSH + login, so MFA belongs on the host/SSH layer, not in this codebase.
