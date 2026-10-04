# 2. Database and migrations

**Depends on:** 0

## Todo

- [ ] Choose a small single-server database; consider SQLite if the agreed concurrency and backup needs fit.
- [ ] Persist users, invites, sessions, estimate metadata, and optional animal records with ownership and timestamps.
- [ ] Create versioned migrations, constraints, bounded queries, and deletion rules.
- [ ] Keep optional stored photos separate from database estimate metadata.

- [ ] Record schema and estimate method/prompt versions; store canonical units and distinguish measurement time from record creation time.
- [ ] Define transaction boundaries and uniqueness rules so retries cannot create duplicate history records or leave partially saved batches.

## Completion check

Accounts and history survive restarts and migrations preserve existing data.

