# 10. Usage limits and spending controls

**Depends on:** 3, 5, 9

## Todo

- [x] Set simple per-user and per-IP login/upload/inference limits suitable for two people. (Daily per-user photo quota, per-IP/account login+invite+recovery limiters, 20 MiB bodies, connection cap — all tested.)
- [x] Count batch items against usage limits; bound queued work, storage, request sizes, and timeouts. (Each photo item bumps the daily counter; tape free; batchq test. Queue bounded by connection cap + inference gate.)
- [x] Add provider spending alerts and a way for the operator to pause inference. (Pause switch: env + operator API + UI, tested. Spending signals: usage endpoint, audit log, monitoring table in docs/operations.md. No push-notification infra — the operator checks usage; suitable for two users.)
- [x] Trust client IP forwarding only from the configured proxy; give clear retry/limit messages. (AIF_TRUSTED_PROXIES, tested incl. untrusted-header bucketing; quota/paused/busy messages user-friendly.)
- [x] Skip billing, paid plans, and subscription infrastructure for this two-user service. (No billing code exists anywhere.)

- [x] Define provider-outage behavior and a bounded retry budget; show an explicit unavailable state or labelled placeholder, and never save a placeholder as a successful AI measurement. (Single retry, explicit 502, pause → 503, placeholder flag + labels, fallback rows stored as placeholder=true; tested.)

## Completion check

Unexpected traffic cannot create unbounded provider spending or exhaust the server.

