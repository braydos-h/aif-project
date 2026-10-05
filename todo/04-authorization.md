# 4. Private access and user isolation

**Depends on:** 2, 3

## Todo

- [x] Require authentication for estimation, history, exports, photos, and job status.
- [x] Enforce ownership server-side for every read, update, delete, and export.
- [x] Decide whether animal records are private or explicitly shared between the two users; implement sharing only with defined permissions.
- [x] Restrict administrator functions and operator metrics; scope caches so private results cannot leak between accounts.

## Completion check

Tests with both accounts prove user isolation and administrator permission boundaries.

