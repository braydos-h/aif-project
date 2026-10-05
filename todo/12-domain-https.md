# 12. Domain, HTTPS, and firewall

**Depends on:** 11

## Todo

- [ ] Configure domain DNS, HTTPS reverse proxy, certificate renewal, and HTTP-to-HTTPS redirect. (Repo-side DONE: deploy/Caddyfile with auto-HTTPS + redirect + renewal alerts, docs/deployment.md. External DNS + live renewal verification still required — see below.)
- [x] Expose only needed public ports; keep backend/database/operator endpoints private. (Backend binds 127.0.0.1, firewall guidance in docs, operator CLI is localhost-only.)
- [x] Align proxy upload/time limits and trusted forwarded headers with the Rust server. (20 MB body, 30/90 s timeouts, X-Forwarded-For + AIF_TRUSTED_PROXIES; forwarding bucket test passes.)
- [ ] Enable HSTS after validating domain HTTPS behavior.
- [ ] Test external access on phone mobile data, including rejection of unauthenticated API calls.

## Completion check

The domain works securely outside the local network and certificate renewal is verified.

## Implementation status (2026-10-05)

Repo-side proxy work is done. Irreducibly external: (1) point real DNS at the host; (2) `systemctl reload caddy`, confirm `https://<domain>` + HTTP→HTTPS redirect on phone mobile data; (3) confirm unauthenticated API calls fail; (4) validate HTTPS end to end, THEN enable the commented HSTS line; (5) re-verify after the first automatic certificate renewal. Evidence required: dated curl/phone screenshots + renewal log lines before checking the remaining boxes.
