# Invitation and recovery delivery

Transactional email is **not configured** for this service. Delivery is
secure operator relay instead (documented here, implemented in the CLI
and operator panel, tested end to end).

## Inviting the two users

1. Operator mints a single-use link (valid `AIF_INVITE_DAYS`, default 7):

   ```sh
   sudo -u aif AIF_DATA_DIR=/var/lib/aif /opt/aif/aif-backend \
     --create-invite user@example.com --role user
   ```

   or WebUI **Account → Operator → Create an invite**. The link is shown
   **once** as `https://<domain>/#invite=<token>` (fragment: the token
   never travels to the server in the URL and never appears in logs).
2. Operator sends it over a private channel (in person, Signal, etc.).
3. The recipient opens it, picks a display name + password (10+
   characters), and is logged in immediately. Used, expired, revoked, or
   unknown links fail with a plain message; reuse fails safely.
4. Invites bind to the issued email; the service caps at **two active
   accounts** — a third acceptance is refused until an account is removed.
5. Replacement: revoke the stale invite
   (`--revoke-invite <id>` or the operator panel) and mint a fresh one.
   Resend attempts are bounded (3 recovery requests per account per day;
   invites are one-shot by design).

## If a user loses access

1. User opens **Recover access**, enters their email, and contacts the
   operator. The response is identical whether or not the email exists
   (no enumeration).
2. Operator mints a single-use 24 h recovery link:

   ```sh
   sudo -u aif AIF_DATA_DIR=/var/lib/aif /opt/aif/aif-backend \
     --create-recovery user@example.com
   ```

   and relays it privately. Using it sets a new password and revokes
   **all** of that account's sessions.

## If the operator loses access

The operator has host (SSH) access by definition: run
`--create-recovery operator@example.com` (or `--list-users` to confirm
the address) on the host and relay the link to yourself over a private
channel. If host access is also lost, follow the hosting provider's
console recovery, then rotate `OLLAMA_API_KEY` and user passwords.

## Verification

- Accept both invites on phone mobile data (HTTPS origin links built
  from `AIF_PUBLIC_ORIGIN`, never the `Host` header).
- Exercise expiry (wait or mint-then-revoke), reuse, and revocation;
  each must fail with its documented code.
- Confirm no token appears in `journalctl -u aif-backend`, Caddy logs,
  or the audit log (audit carries invite ids and emails only).
