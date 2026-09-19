# Access Log for Logins and Downloads

## Status

Done (2026-09-19)

## Summary

Emit nginx-style combined-log lines for logins (success and failure) and for
every file download outcome, into the existing tracing stream. A new opt-in
`trustProxy` config flag controls whether the client IP is read from
`X-Forwarded-For` (behind a trusted reverse proxy) or taken from the peer
socket.

## Problem Statement

Operators have no auditable record of who signed in and which files were
downloaded. The generic tower-http request tracing logs routes, not the
application meaning, so a download of a specific file or a repeated failed
login is not visible. This blocks diagnosis of access misuse and leaves no
forensic trail.

## User Story

As an operator, I want a per-event access log line for each successful and
failed login and each file download, so that I can audit who did what and when.

## Scope

- In scope: a combined-log-style line per login outcome (success and failure).
- In scope: a combined-log-style line per download outcome produced by the
  download handler (200, 206, 304, 416, and the handler's 400/404/503).
- In scope: client IP resolution with an opt-in `trustProxy` flag.
- In scope: lines emitted through the existing tracing stream at `info` level
  under a dedicated target.

## Non-Goals

- Not in scope: a separate access-log file or log rotation.
- Not in scope: JSON or other machine-parseable structured logging.
- Not in scope: access-gate denial lines (a hidden-path 404 returns before the
  download handler runs), directory listing or delete events.
- Not in scope: keeping access logs visible when an operator sets a filter
  that excludes `info` level events.

## UX / Flow

1. A user sign-in completes at `/api/v1/auth/callback`. On success, after the
   session cookie is minted, one line is logged with the user identity and
   status 302. On failure, one line is logged with the failure status
   (401 or 503) and no user.
2. A user requests `/api/v1/download?p=<path>`. One line is logged per outcome
   with the served status and byte count (200 = file size, 206 = range
   length, otherwise 0; a HEAD logs 0 because axum routes HEAD to the GET
   handler with the body removed).
3. Log lines go to the existing stdout stream as `tracing::info!` events.

## Technical Notes

- New `backend/src/access_log.rs` module: pure `client_ip`, `user_agent` and
  line-formatting helpers (unit-testable) plus `log_login_success`,
  `log_login_failure` and `log_download` emitters that log via `tracing::info!`
  with target `yafm::access`.
- `backend/src/main.rs` switches the serve call to
  `into_make_service_with_connect_info::<SocketAddr>()` so handlers can read
  the peer socket. Handlers read it as `Option<Extension<ConnectInfo<SocketAddr>>>`
  so existing router tests, which use `oneshot` without connect info, still pass.
- `backend/src/lib.rs` adds the `trustProxy` config flag (default `false`) to
  the config parse and an `AppConfig` field plus a `get_trust_proxy()` getter.
- Login success is logged inside `callback_response` (where the session
  claims are in scope); login failure is logged in the `callback` handler
  from the returned `AuthFlowError`.
- The download handler is wrapped in `download_inner`; the wrapper derives the
  status and bytes (from `Content-Length` on 200/206) from the outcome and
  logs once per request. A HEAD logs 0 bytes: axum routes HEAD to the GET
  handler with the body removed, so the handler promises a Content-Length
  while transferring nothing (nginx parity). A client that disconnects
  mid-stream still logs the promised length; the count is not the number of
  bytes actually written.
- Every user-controlled field (client IP, user, requested path, referer,
  user agent) is escaped at render time: `"` and `\` render as their escaped
  form, and control characters render as `\xHH` (code points up to U+00FF)
  or `\uHHHH` (above), so a forged `p=foo%0Abar` cannot split the log line.
- The user field is quoted because the display name is free-form (anything
  the IdP sends), like the referer and user-agent; the status and byte
  fields stay unquoted, and a display name with spaces cannot shift the
  following columns.
- The line is nginx-shaped, not a drop-in combined-log line: the event field
  carries an event name (`LOGIN`, `DOWNLOAD <path>`) instead of the HTTP
  request line, and the timestamp is RFC3339 instead of the CLF form, so
  classic combined-log consumers need a custom format (goaccess and fail2ban
  both support one).

## Security Considerations

- Client IP is the peer socket by default. `X-Forwarded-For` is only trusted
  when the operator sets `trustProxy: true`, so a spoofed header cannot
  appear in logs unless a reverse proxy is explicitly in front. Under
  `trustProxy`, the left-most hop is logged only when it parses as an IP
  address (MDN's X-Forwarded-For guidance: spoofed values may not be actual
  addresses), which bounds the field to 45 characters; a hop carrying a port
  or garbage falls back to the peer socket IP. Private-range hops are not
  filtered, because a deployment on a LAN would lose legitimate audit data.
- Log lines carry the user display name and the requested relative path; no
  host filesystem path, OIDC client secret, signing key, or `returnTo` is
  logged.
- Quotes, backslashes and control characters (C0, DEL, C1, and the Unicode
  line/paragraph separators) in user-controlled fields are escaped at render
  time, so a forged path or header cannot inject extra log lines or break
  the combined-log field quoting.
- The login failure line carries no user identity because the identity is not
  established when the flow fails.

## Acceptance Criteria

- [x] A successful login emits one `LOGIN` line with the user identity and
      status 302.
- [x] A failed login (token/state rejection and provider-unavailable) emits
      one `LOGIN` line with no user and the matching status.
- [x] A download emits one line per outcome with the served status and byte
      count.
- [x] `trustProxy` defaults to `false`; when false, a stray `X-Forwarded-For`
      is ignored and the peer IP is logged.
- [x] When `trustProxy` is `true`, the left-most `X-Forwarded-For` hop is
      logged, falling back to the peer IP when the header is absent.
- [x] When `trustProxy` is `true`, a left-most hop that does not parse as an
      IP address is rejected and the peer IP is logged.
- [x] A HEAD download logs zero bytes (axum routes HEAD to the GET handler
      with the body removed).
- [x] The user field is quoted, so a display name with spaces cannot shift
      the following columns.
- [x] Existing router tests still pass unchanged.

## Test Plan

- Unit: `client_ip` resolution (peer, trusted forwarded, non-IP hop
  rejection, fallback, absent connect info), `user_agent`, and the
  `Line::render` shape (`backend/src/access_log.rs`).
- Unit: the login failure status mapping (401 vs 503).
- Unit: the escaped combined-log shape (C0/DEL/C1 controls, the Unicode
  line/paragraph separators, and quotes cannot forge a second line).
- Integration: the download wrapper derives the status and bytes for
  200/206/304/416 outcomes, a HEAD download logs zero bytes, and a forged
  newline in the path renders escaped (`backend/tests/access_log.rs`).
- Regression: `cargo test --release --lib`, `npm run check`.
