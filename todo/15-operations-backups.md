# 15. Operations, monitoring, and backups

**Depends on:** 10, 11, 12, 14

## Todo

- [ ] Provide minimal protected operator controls for invitations, revocation, usage, and pausing inference.
- [ ] Log security-sensitive actions and request IDs without photos, credentials, private prompts, or reset tokens.
- [ ] Monitor uptime, provider failures, disk capacity, certificate expiry, and spending.
- [ ] Automate encrypted off-server database/photo backups with defined retention.
- [ ] Perform a restore drill and document updates, rollback, outages, credential compromise, and operator recovery.

- [ ] Document provider-key rotation, dependency/model update review, and expiry or payment failures for the domain, host, and provider account.
- [ ] Keep a tested service closure procedure covering user exports, invitation shutdown, credential revocation, and deletion under the agreed retention policy.

## Completion check

Alerts reach the operator and a tested restore recovers the two-user service.

