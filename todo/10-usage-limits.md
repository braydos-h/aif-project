# 10. Usage limits and spending controls

**Depends on:** 3, 5, 9

## Todo

- [ ] Set simple per-user and per-IP login/upload/inference limits suitable for two people.
- [ ] Count batch items against usage limits; bound queued work, storage, request sizes, and timeouts.
- [ ] Add provider spending alerts and a way for the operator to pause inference.
- [ ] Trust client IP forwarding only from the configured proxy; give clear retry/limit messages.
- [ ] Skip billing, paid plans, and subscription infrastructure for this two-user service.

- [ ] Define provider-outage behavior and a bounded retry budget; show an explicit unavailable state or labelled placeholder, and never save a placeholder as a successful AI measurement.

## Completion check

Unexpected traffic cannot create unbounded provider spending or exhaust the server.

