# 16. Tests, staging, and release checks

**Depends on:** 5, 11, 15

## Todo

- [x] Automate relevant Rust tests, format checks, release build, and dependency checks. (CI: fmt, clippy -D warnings, secrets/unsafe-HTML guards, tests, release build, node checks, synthetic aif-eval gate, deploy sh -n.)
- [x] Test invitation limits/reuse, login/logout/recovery, CSRF, isolation, exports/deletion, quotas, and unsafe production overrides. (26 integration tests + unit tests; 179 total green.)
- [ ] Use isolated staging data/secrets; smoke-test HTTPS and both phone account journeys.
- [x] Document migration backups, deployment order, and rollback or forward repair. (deploy/update.sh + rollback.sh + docs/deployment.md; migrations are additive-only.)
- [x] Keep deployment approval separate from planning; this roadmap authorizes no live deployment. (No deployment performed or authorized here.)

- [x] Verify spreadsheet-safe exports, duplicate-request handling, database rollback on failed writes, and restart recovery for any jobs. (csv unit tests + export shape test, idempotent replay test, atomic-batch unit test, restart persistence test; no durable jobs by decision.)
- [ ] Check keyboard-only navigation, screen-reader labels/status updates, zoomed layouts, and clear error recovery on both phone platforms.

## Completion check

A repeatable release process verifies private access and preserves durable data.

## Implementation status (2026-10-05)

Repo-side automation and verification are done. Irreducibly external: (1) staging run with isolated data/secrets + HTTPS smoke test and both phone account journeys; (2) keyboard-only, screen-reader, and zoomed-layout passes on real iOS/Android hardware. Evidence required: dated staging checklist + device notes before checking those boxes.
