# 12. Domain, HTTPS, and firewall

**Depends on:** 11

## Todo

- [ ] Configure domain DNS, HTTPS reverse proxy, certificate renewal, and HTTP-to-HTTPS redirect.
- [ ] Expose only needed public ports; keep backend/database/operator endpoints private.
- [ ] Align proxy upload/time limits and trusted forwarded headers with the Rust server.
- [ ] Enable HSTS after validating domain HTTPS behavior.
- [ ] Test external access on phone mobile data, including rejection of unauthenticated API calls.

## Completion check

The domain works securely outside the local network and certificate renewal is verified.

