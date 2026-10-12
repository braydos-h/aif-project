# 0. Setup and scope

**Depends on:** None

## Todo

- [ ] Choose the server, domain, budget, and operator for a two-user invite-only service.
- [x] Keep Rust and plain HTML/CSS/JavaScript; choose a small durable database and authentication approach. (SQLite via rusqlite/bundled, Argon2id + random session tokens; docs/security.md)
- [x] Define two invited accounts, private per-user data, and whether either user is also the administrator. (Two-active-account cap, operator role, per-user ownership; docs/security.md)
- [x] Separate local, staging, and production configuration and secrets; keep local launchers working. (AIF_REQUIRE_AUTH/AIF_PRODUCTION, .env.example, production startup validation; start_gui/start.sh untouched, integration suite green)

- [ ] Agree on expected usage, supported cattle/measurement cases, acceptable response time, and a measurable accuracy target before calling photo estimates production-ready.
- [ ] Record who owns the domain, hosting, provider account, and recovery credentials; define a service closure plan if the operator stops maintaining it.

## Completion check

Architecture, account policy, and deployment choices are recorded.

## Implementation status (2026-10-05)

Repo-side scope decisions are implemented and tested. Remaining unchecked items need the human operator: pick the server/domain/budget, agree measurable accuracy targets on real data (framework in eval/, gate BLOCKED), and record ownership/recovery credentials (closure procedure itself is in docs/operations.md).
