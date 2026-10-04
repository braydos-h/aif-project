# 6. Persistent user history

**Depends on:** 2, 4, 5

## Todo

- [ ] Replace session-only history with account-owned durable estimates, timestamps, inputs, source/model, units, and weight ranges.
- [ ] Add pagination, detail views, filters, delete, and owned CSV export.
- [ ] Avoid storing base64 images, credentials, or unnecessary raw model responses.
- [ ] Label deterministic fallback results as placeholders; retain uncertainty and dosing warnings in saved/exported results.

- [ ] Make CSV exports safe for spreadsheet use: quote delimiters/newlines and neutralize formula-like user fields such as animal names; preserve numeric measurement columns.
- [ ] Display measurement dates in the user's timezone while storing unambiguous timestamps; preserve original estimate/model versions when settings change.

## Completion check

Each user can retrieve their own history on another device after a server restart.

