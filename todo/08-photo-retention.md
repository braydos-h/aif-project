# 8. Photo privacy and retention

**Depends on:** 2, 4, 5

## Todo

- [ ] Decide whether photos are transient only; default to no retained photos unless photo history is wanted.
- [ ] If retained, use private storage with opaque IDs and authorized downloads; never construct filesystem paths from URL segments.
- [ ] Validate content/size, bound decoded resources, remove location metadata from retained processed photos, and set storage limits.
- [ ] Define expiry, deletion, orphan cleanup, and backup retention.
- [ ] Explain when photos are sent to the AI provider and what is retained.

## Completion check

Only authorized users can access retained photos and deletion/retention cleanup works.

