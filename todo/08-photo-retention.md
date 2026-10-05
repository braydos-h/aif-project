# 8. Photo privacy and retention

**Depends on:** 2, 4, 5

## Todo

- [x] Decide whether photos are transient only; default to no retained photos unless photo history is wanted. (Default transient-only; OPTIONAL opt-in retention implemented behind AIF_RETAIN_PHOTOS=1 with per-estimate consent. Recorded in docs/security.md and docs/privacy.md.)
- [x] If retained, use private storage with opaque IDs and authorized downloads; never construct filesystem paths from URL segments. (IMPLEMENTED: <data_dir>/photos/<32-hex-id> mode 0600, owner-only GET, URL ids allow-list validated, private cache headers; photo lifecycle test proves cross-user 404.)
- [x] Validate content/size, bound decoded resources, remove location metadata from retained processed photos, and set storage limits. (IMPLEMENTED: magic-byte + 20 MiB validation before write, JPEG APP1/Exif + PNG eXIf stripping unit-tested, per-user quota AIF_PHOTO_QUOTA_MB with 429 on breach.)
- [x] Define expiry, deletion, orphan cleanup, and backup retention. (IMPLEMENTED: TTL expiry AIF_PHOTO_TTL_DAYS, history/account deletion removes rows + files, startup/periodic sweep of expired/orphan/missing both directions, DB backups exclude photos by design; tested.)
- [x] Explain when photos are sent to the AI provider and what is retained. (docs/privacy.md + Account view photo-policy line: sent to provider for the estimate, never stored.)

## Completion check

Only authorized users can access retained photos and deletion/retention cleanup works.

