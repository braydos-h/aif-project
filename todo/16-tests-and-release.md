# 16. Tests, staging, and release checks

**Depends on:** 5, 11, 15

## Todo

- [x] Automate relevant Rust tests, format checks, release build, and dependency checks. (CI: fmt, clippy -D warnings, secrets/unsafe-HTML guards, tests, release build, node checks, synthetic aif-eval gate, deploy sh -n.)
- [x] Test invitation limits/reuse, login/logout/recovery, CSRF, isolation, exports/deletion, quotas, and unsafe production overrides. (26 integration tests + unit tests; 179 total green.)
- [ ] Use isolated staging data/secrets; smoke-test HTTPS and both phone account journeys.
- [x] Document migration backups, deployment order, and rollback or forward repair. (deploy/update.sh + rollback.sh + docs/deployment.md; migrations are additive-only.)
- [x] Keep deployment approval separate from planning; this roadmap authorizes no live deployment. (No deployment performed or authorized here.)

- [x] Verify spreadsheet-safe exports, duplicate-request handling, database rollback on failed writes, and restart recovery for jobs. (CSV tests + export shape test, idempotent replay and full-queue replay tests, atomic-batch test, job restart persistence, owner-scoped upload-batch retry/cleanup coverage.)
- [x] Add an automated browser journey for invite acceptance, login, direct routes, animal records, two-photo background upload, history, mobile navigation, and logout. (web/tests/journeys.test.mjs; staging/device evidence remains open.)
- [ ] Check keyboard-only navigation, screen-reader labels/status updates, zoomed layouts, and clear error recovery on both phone platforms.

## Completion check

A repeatable release process verifies private access and preserves durable data.

## Implementation status (2026-10-08)

Repo-side automation covers Rust tests, formatting, Clippy, release builds, JavaScript syntax/security guards, and a Chromium browser journey. Irreducibly external: (1) staging run with isolated data/secrets + HTTPS smoke test and both invited users' journeys; (2) keyboard-only, screen-reader, interrupted-upload, and zoomed-layout passes on real iOS/Android hardware. Evidence required: dated staging checklist + device notes before checking those boxes.
