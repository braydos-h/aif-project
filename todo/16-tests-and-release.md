# 16. Tests, staging, and release checks

**Depends on:** 5, 11, 15

## Todo

- [ ] Automate relevant Rust tests, format checks, release build, and dependency checks.
- [ ] Test invitation limits/reuse, login/logout/recovery, CSRF, isolation, exports/deletion, quotas, and unsafe production overrides.
- [ ] Use isolated staging data/secrets; smoke-test HTTPS and both phone account journeys.
- [ ] Document migration backups, deployment order, and rollback or forward repair.
- [ ] Keep deployment approval separate from planning; this roadmap authorizes no live deployment.

- [ ] Verify spreadsheet-safe exports, duplicate-request handling, database rollback on failed writes, and restart recovery for any jobs.
- [ ] Check keyboard-only navigation, screen-reader labels/status updates, zoomed layouts, and clear error recovery on both phone platforms.

## Completion check

A repeatable release process verifies private access and preserves durable data.

