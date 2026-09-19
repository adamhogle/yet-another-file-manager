# Access Log and Trusted Client IP

## Status

Accepted

## Context

YAFM is deployed behind a reverse proxy (nginx) that terminates TLS, so the
socket the backend sees is the proxy, not the browser. An nginx-style access
log for logins and downloads needs the real client IP, which is only available
through a forwarded header (typically `X-Forwarded-For`). Trusting that header
blindly lets a client spoof an arbitrary IP into the audit trail, which
defeats the purpose of an access log.

## Decision

- Emit nginx-style combined-log lines for login outcomes and download
  outcomes as `tracing::info!` events under target `yafm::access`, so they
  flow through the existing stdout stream. Access logging is always on.
- Add an opt-in `trustProxy` config flag, default `false`. When `false`, the
  logged client IP is the peer socket and a stray `X-Forwarded-For` is
  ignored. When `true`, the left-most `X-Forwarded-For` hop is logged only
  when it parses as an IP address, falling back to the peer socket IP when
  the header is absent or the hop is not an IP (MDN's X-Forwarded-For
  guidance: the left-most hop is untrustworthy and spoofed values may not be
  actual addresses). Private-range hops are not filtered: deployments on a
  LAN would lose legitimate audit data.
- Enable peer-socket extraction by serving with
  `into_make_service_with_connect_info::<SocketAddr>()`, and read it in
  handlers as `Option<ConnectInfo<SocketAddr>>` so the test harness, which
  uses `oneshot` without connect info, is unaffected.

## Alternatives Considered

1. Always trust `X-Forwarded-For`. Rejected: a spoofed header would poison the
   audit trail, and YAFM may be reached directly (not behind a proxy) in some
   deployments.
2. Write a dedicated access-log file. Rejected for this pass: the operator
   already collects the stdout stream; a separate file adds file handling,
   rotation, and a second sink for no current need.
3. Emit structured JSON events. Rejected: the operator asked for nginx-style
   text lines.

## Consequences

- Access lines appear by default because the filter includes `info`; an
  operator who sets `RUST_LOG` to a level above `info` will suppress them
  (documented non-goal).
- Behind a proxy, the operator must set `trustProxy: true` or the log shows
  the proxy IP.
- The `yafm::access` target lets operators filter or route access lines
  independently of app logs.

## Security / Operations Impact

- Default posture is safe: forwarded headers are ignored until `trustProxy`
  is enabled by an operator who knows a trusted proxy forwards them.
- No secrets or host paths are logged; only user display name, requested
  relative path, status, and byte count.
- If the proxy does not sanitize `X-Forwarded-For`, a client can still inject
  leading hops; the left-most hop is logged only when it parses as an IP
  address, so injected garbage, ports and delimiters never reach the audit
  trail and the field is bounded to 45 characters. Quotes, backslashes and
  control characters in every user-controlled field are escaped at render
  time, so a forged path or header cannot split a log line.
