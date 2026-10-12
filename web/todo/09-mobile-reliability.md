# 9. Mobile uploads and reliable estimates

**Depends on:** 1, 6, 8

## Todo

- [x] Test camera capture, library selection, orientation, image resizing, and supported formats including HEIC handling or clear guidance. (capture attribute kept, in-browser resize + EXIF orientation kept, explicit HEIC rejection message with JPEG guidance. Real-device checks need physical phones — see 16/17.)
- [x] Preserve accessible tap targets, tape inputs, dark mode, charts, and progress on iOS and Android. (44px targets, dark-mode account styles, progress + live regions kept.)
- [x] Handle slow connections, backgrounded tabs, reconnects, provider timeouts, batch partial failures, and safe retry. (Timeouts kept, per-item batch isolation + retry, online/offline handling kept, idempotency keys make retries safe.)
- [x] Bound inference concurrency; add idempotency and recoverable job status if needed to prevent duplicate work after disconnects. (IMPLEMENTED: durable jobs — POST /api/jobs queues through the same pipeline (quotas/pause/idempotency apply), FIFO worker, pollable status, cancel; restart requeues with exactly-once saves. Plus foreground idempotency-key replay. Tested incl. pause+restart.)
- [x] Keep installable PWA support optional; do not cache private account data by default. (No service worker, no app-cache, no browser storage of private data; static assets use immutable cache headers only; background jobs + Estimate-in-background cover the reconnect story instead.)

- [x] Add practical photo and tape guidance: full animal side view, suitable lighting, measurement landmarks, supported units, and clear errors for unsuitable images. (Good-photo-tips section kept, tape landmarks kept, HEIC/validation errors specific.)
- [x] Define success, failure, cancellation, and expiry for any durable jobs; reconcile interrupted work after restart and save each completed result once. (IMPLEMENTED: queued/active/success/failed/cancelled/expired states, bounded backoff retries, boot requeue, TTL pruning, idempotent exactly-once completion; worker + DB unit tests and job lifecycle/restart integration tests.)
- [x] Warn before leaving with unsaved inputs and preserve recoverable results without storing private photos or credentials in browser storage. (beforeunload guard on unsent selections; saved results persist server-side for logged-in users.)

## Completion check

Both users can estimate and retrieve results over mobile data, including after interruption.

