# 9. Mobile uploads and reliable estimates

**Depends on:** 1, 6, 8

## Todo

- [ ] Test camera capture, library selection, orientation, image resizing, and supported formats including HEIC handling or clear guidance.
- [ ] Preserve accessible tap targets, tape inputs, dark mode, charts, and progress on iOS and Android.
- [ ] Handle slow connections, backgrounded tabs, reconnects, provider timeouts, batch partial failures, and safe retry.
- [ ] Bound inference concurrency; add idempotency and recoverable job status if needed to prevent duplicate work after disconnects.
- [ ] Keep installable PWA support optional; do not cache private account data by default.

- [ ] Add practical photo and tape guidance: full animal side view, suitable lighting, measurement landmarks, supported units, and clear errors for unsuitable images.
- [ ] Define success, failure, cancellation, and expiry for any durable jobs; reconcile interrupted work after restart and save each completed result once.
- [ ] Warn before leaving with unsaved inputs and preserve recoverable results without storing private photos or credentials in browser storage.

## Completion check

Both users can estimate and retrieve results over mobile data, including after interruption.

