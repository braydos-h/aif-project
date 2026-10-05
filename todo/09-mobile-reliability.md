# 9. Mobile uploads and reliable estimates

**Depends on:** 1, 6, 8

## Todo

- [x] Test camera capture, library selection, orientation, image resizing, and supported formats including HEIC handling or clear guidance. (capture attribute kept, in-browser resize + EXIF orientation kept, explicit HEIC rejection message with JPEG guidance. Real-device checks need physical phones — see 16/17.)
- [x] Preserve accessible tap targets, tape inputs, dark mode, charts, and progress on iOS and Android. (44px targets, dark-mode account styles, progress + live regions kept.)
- [x] Handle slow connections, backgrounded tabs, reconnects, provider timeouts, batch partial failures, and safe retry. (Timeouts kept, per-item batch isolation + retry, online/offline handling kept, idempotency keys make retries safe.)
- [x] Bound inference concurrency; add idempotency and recoverable job status if needed to prevent duplicate work after disconnects. (InferenceGate + daily quotas + pause; idempotency-key replay tested. DECISION: no durable job queue — estimates are synchronous, batch items isolate failures, restart cannot duplicate results; recorded in docs/security.md.)
- [x] Keep installable PWA support optional; do not cache private account data by default. (No service worker, no app-cache, no browser storage of private data; static assets use immutable cache headers only.)

- [x] Add practical photo and tape guidance: full animal side view, suitable lighting, measurement landmarks, supported units, and clear errors for unsuitable images. (Good-photo-tips section kept, tape landmarks kept, HEIC/validation errors specific.)
- [x] Define success, failure, cancellation, and expiry for any durable jobs; reconcile interrupted work after restart and save each completed result once. (N/A by no-durable-jobs decision: synchronous estimates + idempotency; restart persistence covered by history_restart test.)
- [x] Warn before leaving with unsaved inputs and preserve recoverable results without storing private photos or credentials in browser storage. (beforeunload guard on unsent selections; saved results persist server-side for logged-in users.)

## Completion check

Both users can estimate and retrieve results over mobile data, including after interruption.

