# Two-user invite-only hosting roadmap

Goal: host Cow Weight Estimator on a domain so two invited users can use it remotely on their phones. The address is internet-accessible; accounts, estimates, photos, and history are private. There is no public registration.

Planning only. Implement roughly in numbered order, respecting dependencies; item 18 is an estimate-quality gate to complete before item 17 launches the service. Login screens are item 1; database and authentication follow in items 2 and 3. Animal records are optional; photo retention and durable jobs depend on the recorded product decisions. No billing, native app, public signup, or large-scale infrastructure is required.

## Ordered tasks

- [ ] [0. Setup and scope](00-setup.md)
- [x] [1. WebUI login and account screens](01-webui-login.md)
- [x] [2. Database and migrations](02-database.md)
- [x] [3. Invite-only authentication and sessions](03-invite-authentication.md)
- [x] [4. Private access and user isolation](04-authorization.md)
- [x] [5. Production API and browser security](05-production-security.md)
- [x] [6. Persistent user history](06-user-history.md)
- [x] [7. Animal records and trends](07-animal-records.md)
- [x] [8. Photo privacy and retention](08-photo-retention.md)
- [x] [9. Mobile uploads and reliable estimates](09-mobile-reliability.md)
- [x] [10. Usage limits and spending controls](10-usage-limits.md)
- [ ] [11. Server packaging and deployment](11-server-deployment.md)
- [ ] [12. Domain, HTTPS, and firewall](12-domain-https.md)
- [x] [13. Invitation and recovery delivery](13-invitation-recovery-delivery.md)
- [x] [14. Privacy and account controls](14-privacy-account-controls.md)
- [ ] [15. Operations, monitoring, and backups](15-operations-backups.md)
- [ ] [16. Tests, staging, and release checks](16-tests-and-release.md)
- [ ] [17. Two-user launch checklist](17-launch.md)
- [ ] [18. Estimate quality and user guidance](18-estimate-quality.md)

Preserve the Rust backend, plain browser frontend, local launch path, explicit static routes, image validation, request limits, request IDs, safe DOM rendering, secret redaction, and estimate disclaimers.

