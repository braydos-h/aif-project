# 13. Invitation and recovery delivery

**Depends on:** 3, 12

## Todo

- [ ] Implement the selected invitation/recovery delivery method; transactional email is optional if secure operator delivery is chosen.
- [ ] If using email, configure sender-domain records and test real delivery.
- [ ] Build links from the configured HTTPS origin, never an untrusted Host header.
- [ ] Keep invite/reset tokens out of logs; bound expiry, resend attempts, and delivery retries.
- [ ] Document how both users recover access and how the operator recovers administrative access.

## Completion check

Both invited users can activate/recover their accounts and invalid or reused links fail.

