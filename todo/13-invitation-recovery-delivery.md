# 13. Invitation and recovery delivery

**Depends on:** 3, 12

## Todo

- [x] Implement the selected invitation/recovery delivery method; transactional email is optional if secure operator delivery is chosen.
- [x] If using email, configure sender-domain records and test real delivery.
- [x] Build links from the configured HTTPS origin, never an untrusted Host header.
- [x] Keep invite/reset tokens out of logs; bound expiry, resend attempts, and delivery retries.
- [x] Document how both users recover access and how the operator recovers administrative access.

## Completion check

Both invited users can activate/recover their accounts and invalid or reused links fail.

## Implementation status (2026-10-05)

Secure operator relay implemented (CLI + operator panel + docs/recovery.md); email explicitly out of scope. Invite/recovery links are built from AIF_PUBLIC_ORIGIN, tokens never touch logs, expiry/resend/single-use enforced and tested.
