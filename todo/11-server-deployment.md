# 11. Server packaging and deployment

**Depends on:** 0, 2, 5, 8, 9

## Todo

- [x] Provide a reproducible Linux release and choose a service manager or container.
- [x] Run as a non-root service with private backend binding, protected secrets, and durable data volumes.
- [x] Configure restart-on-failure, startup after reboot, graceful shutdown, resource limits, and log rotation.
- [x] Validate required production configuration at startup and document installation/update commands.
- [x] Keep database/storage administration off the public network.

## Completion check

A clean server can run and reboot the application without losing accounts or history.

## Implementation status (2026-10-05)

All repo-side assets exist (deploy/aif-backend.service, install/update/rollback, docs/deployment.md) and scripts pass `sh -n`. Running them against a clean server (first boot, reboot survival, data preservation) needs the real host — verify during staging (16) and record evidence at launch (17).
