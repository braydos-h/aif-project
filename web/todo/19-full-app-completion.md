# 19. Full app completion and acceptance evidence

This tracker follows the user's ordered eight-stage completion plan. Local
feature work and external acceptance are recorded separately so a passing
test suite is not mistaken for staging or production evidence.

| Stage | Scope | Repository status | Acceptance evidence / remaining gate |
| --- | --- | --- | --- |
| 1. Pages and navigation | Explicit Rust page routes, shared shell, responsive menu, browser history, titles, missing-page handling | Implemented | `application_pages_are_explicit_and_detail_ids_are_validated`; browser journey opens and refreshes each listed route. |
| 2. Account access | Invitation-only login, recovery/reset, password change, intended-page return, session probe, operator isolation, logout cleanup | Implemented | Rust invite/login/logout, recovery, password-change and ownership integration tests; browser invite/login/deep-link/logout journey. Run both invited users through staging before launch. |
| 3. Batch uploads | Select/review/submit/track, 20-item cap, per-photo links, owner-scoped batches, partial states, cancel, retry, idempotency, reconnect | Implemented | `upload_batches_keep_partial_jobs_private_and_clear_terminal_payloads`, `failed_batch_item_retries_same_job_and_clears_payload_again`, full-queue replay tests, and browser two-photo mixed-result journey. Interrupted mobile-network retry remains a device check. |
| 4. Records and results | Filtered history/export/details, animal CRUD/archive/scale/trends, source and model labeling | Implemented | History/CSV, animal trend, scale-source, ownership, and export integration tests; browser creates an animal, links two estimates, and opens history and animal detail pages. |
| 5. Interface quality | Shared styles, empty/loading/error states, touch targets, focus, keyboard and mobile navigation, dark mode | Implemented in the WebUI | Existing CSS/static guards and the browser journey's 390 px menu check. Complete keyboard-only, screen-reader, zoom, and real phone checks on iOS and Android. |
| 6. Privacy and operations | Temporary queued-photo storage, terminal cleanup, sanitized backups, retained-photo expiry/deletion, operator limits/status, accurate docs | Implemented in the repository | Terminal and migration cleanup tests; `deploy/backup.sh` clears every job payload in the encrypted snapshot; backup script syntax check. Execute a real backup/restore drill and verify deletion/expiry on staging. |
| 7. Release verification | Rust tests, fmt, Clippy, release build, JS/security checks, browser journeys, staging and device evidence | Automated repository checks implemented | Run log is recorded in `commits.md`; CI runs the same Rust/JS checks and a Chromium journey. HTTPS staging, both real-user journeys, and real iOS/Android evidence remain open. |
| 8. Production readiness | Host/domain/operator/provider/budget, HTTPS/firewall/restart/monitoring, encrypted off-server backups and restore, verified estimate quality | Blocked on external decisions and evidence | Supply the production host/domain, operator contact, provider account/budget, and permissioned cattle photos with verified scale weights and agreed accuracy threshold. Then record staging, restore, spending, and mobile-data evidence before launch. |

## 2026-10-08 local verification

- Rust: `cargo fmt --check`, Clippy with `-D warnings`, release build, and
  `cargo test` passed (101 unit tests + 109 HTTP integration tests).
- Web and operations: `npm ci`, all four JavaScript syntax checks, unsafe-HTML
  and browser-storage guards, deploy-script syntax checks, and `git diff
  --check` passed.
- Browser: the invited-user Playwright journey passed in system Chromium. It
  accepted an invitation, created an animal, submitted two photos, confirmed
  both linked results, opened/reloaded every page route, checked mobile
  navigation and operator isolation, then logged out and returned to the
  requested history page after login.
- Evaluation: the synthetic fixture completed. It is a framework check only;
  it does not establish cattle-weight accuracy.
- A local Playwright browser download was denied by its CDN (HTTP 403), so the
  journey used `/usr/bin/chromium`. CI installs Chromium independently.
- No staging, real-device, production host, restore-drill, or real-data accuracy
  evidence was produced in this workspace. Those gates remain open.

## Completion rule

Stages 1–6 describe implemented product behavior. Stage 7 is complete only
after automated checks and both invited-user staging journeys have evidence.
Stage 8 is complete only after the host, privacy, restore, accuracy, and
mobile-data gates above are demonstrated. Synthetic evaluation fixtures do
not prove estimate accuracy. No production deployment is claimed by this
repository work.
