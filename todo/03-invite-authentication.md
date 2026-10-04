# 3. Invite-only authentication and sessions

**Depends on:** 0, 2

## Todo

- [ ] Let only the operator invite the two approved users; do not expose a public signup endpoint.
- [ ] Use expiring, single-use, cryptographically random invites tied to intended recipients; define replacement/revocation without accidentally allowing a third active account.
- [ ] Use an established authentication provider or maintained password-hashing/session libraries; do not invent cryptography.
- [ ] Use secure HttpOnly session cookies, appropriate SameSite/CSRF protections, expiry, rotation, logout, and revocation.
- [ ] Choose email recovery or a documented secure operator recovery procedure; support administrator MFA where available.
- [ ] Prevent account enumeration and rate-limit login, invite acceptance, and recovery.

## Completion check

Uninvited users cannot create accounts; expired/reused/revoked invites fail; only two approved active users can authenticate.

