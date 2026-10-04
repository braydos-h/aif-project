# 5. Production API and browser security

**Depends on:** 3, 4

## Todo

- [ ] In production reject ordinary-user overrides for ollama_url, ollama_api_key, model, and backend; keep provider secrets on the server.
- [ ] Audit custom HTTP parsing, framing, timeouts, connection limits, and reverse-proxy behavior; decide whether a maintained Rust HTTP stack is required.
- [ ] Enforce CSRF protection, intentional same-origin/CORS policy, security headers, and a compatible Content Security Policy.
- [ ] Preserve request IDs, structured errors, body/image limits, static route allow-lists, secret redaction, and SSRF defenses.
- [ ] Reduce public info/health details and keep metrics private; test malformed traffic and blocked private-network image URLs.

- [ ] Treat model replies as untrusted data: validate finite values, units, ranges, and response sizes before saving or displaying results; image or prompt instructions must not change authorization or server configuration.

## Completion check

Unauthenticated callers cannot spend provider credits or expose internal configuration; security boundary tests pass.

