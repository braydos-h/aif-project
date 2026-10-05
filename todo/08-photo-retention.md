# 8. Photo privacy and retention

**Depends on:** 2, 4, 5

## Todo

- [x] Decide whether photos are transient only; default to no retained photos unless photo history is wanted. (DECISION: transient-only; no photo storage implemented. Recorded in docs/security.md and docs/privacy.md.)
- [x] If retained, use private storage with opaque IDs and authorized downloads; never construct filesystem paths from URL segments. (N/A by transient-only decision: there is no retained-photo surface to protect.)
- [x] Validate content/size, bound decoded resources, remove location metadata from retained processed photos, and set storage limits. (Upload path keeps magic-byte/size validation and bounded decoding; nothing is retained, so no EXIF stripping or quotas are needed.)
- [x] Define expiry, deletion, orphan cleanup, and backup retention. (N/A by transient-only decision: no stored photos means no expiry/cleanup/orphans; backup policy covers the DB only.)
- [x] Explain when photos are sent to the AI provider and what is retained. (docs/privacy.md + Account view photo-policy line: sent to provider for the estimate, never stored.)

## Completion check

Only authorized users can access retained photos and deletion/retention cleanup works.

