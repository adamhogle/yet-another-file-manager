//! Nginx-style access logging for logins and file downloads.
//!
//! Emits one combined-log-style line per login attempt (success or failure)
//! and per download outcome into the existing tracing stream under the
//! `yafm::access` target. The client IP resolution honors an opt-in
//! `trustProxy` config flag: when off (the default) the peer socket IP is
//! logged and a spoofed `X-Forwarded-For` is ignored; when on, the left-most
//! `X-Forwarded-For` hop (the client behind a trusted reverse proxy) is
//! logged, falling back to the peer IP when the header is absent.

use std::fmt::Write as _;
use std::net::{IpAddr, SocketAddr};

use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, StatusCode, header};
use chrono::Utc;

use crate::auth::{AuthFlowError, SessionClaims};

/// The tracing target access lines are emitted under, so operators can filter
/// or route them independently of application logs.
pub const ACCESS_LOG_TARGET: &str = "yafm::access";

const UNKNOWN: &str = "-";

/// Resolves the client IP to log. When `trust_proxy` is true, the left-most
/// `X-Forwarded-For` hop (the client behind a trusted reverse proxy) is used
/// when it parses as an IP address; otherwise (when the header is absent,
/// blank, or carries a value that is not an IP) the peer socket IP. Nothing
/// spoofed or malformed reaches the log unless the operator enabled the
/// trust, and a validated IP is at most 45 characters.
pub fn client_ip(
    connect_info: Option<&ConnectInfo<SocketAddr>>,
    headers: &HeaderMap,
    trust_proxy: bool,
) -> String {
    if trust_proxy
        && let Some(value) = headers
            .get(header::HeaderName::from_static("x-forwarded-for"))
            .and_then(|value| value.to_str().ok())
        && let Some(first) = value.split(',').next()
    {
        // The left-most hop stays untrustworthy even under `trustProxy`: a
        // spoofed value may not be an actual address (MDN's X-Forwarded-For
        // guidance). Only a value that parses as an IP address is logged,
        // which bounds the field; anything else falls back to the peer
        // socket IP below. The first header's first entry is the left-most
        // hop of the combined list, so `get` alone is the right read.
        let hop = first.trim();
        if hop.parse::<IpAddr>().is_ok() {
            return hop.to_string();
        }
    }
    match connect_info {
        Some(connect_info) => connect_info.0.ip().to_string(),
        None => UNKNOWN.to_string(),
    }
}

/// One header value as sent, or "-" when the header is absent or not UTF-8.
fn header_or_dash(headers: &HeaderMap, name: header::HeaderName) -> String {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or(UNKNOWN)
        .to_string()
}

/// The `User-Agent` value as sent, or "-" when absent.
pub fn user_agent(headers: &HeaderMap) -> String {
    header_or_dash(headers, header::USER_AGENT)
}

/// The referer value as sent, or "-" when absent.
pub fn referer(headers: &HeaderMap) -> String {
    header_or_dash(headers, header::REFERER)
}

/// The user label for a log line: the session display name when present,
/// otherwise the subject (the immutable identity). "-" when there is no
/// identity at all (an unauthenticated download or a failed login).
pub fn identity_label(claims: Option<&SessionClaims>) -> String {
    match claims {
        Some(claims) if !claims.display_name.is_empty() => claims.display_name.clone(),
        Some(claims) => claims.subject.clone(),
        None => UNKNOWN.to_string(),
    }
}

fn now_iso() -> String {
    Utc::now().to_rfc3339()
}

fn emit(line: &str) {
    tracing::info!(target: ACCESS_LOG_TARGET, "{line}");
}

/// Escapes the characters that would break the combined-log shape: `"` and
/// `\\` inside a quoted field, and control characters (a newline would forge
/// extra log lines). Code points up to U+00FF render as `\\xHH`, code points
/// above as `\\uHHHH`: `\\x` escapes are always two digits and `\\u` escapes
/// four, so neither form is ambiguous on re-parse. Multi-byte printable
/// characters pass through, so display names and paths stay readable.
fn escape_field(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        let code = character as u32;
        if character == '"' {
            escaped.push_str("\\\"");
        } else if character == '\\' {
            escaped.push_str("\\\\");
        } else if code < 0x20
            || code == 0x7f
            || (0x80..=0x9f).contains(&code)
            || code == 0x2028
            || code == 0x2029
        {
            // C0, DEL and C1 controls, plus the Unicode line and paragraph
            // separators, break the line shape just like a raw newline: some
            // log viewers render U+2028/U+2029 as line breaks.
            if code <= 0xff {
                let _ = write!(escaped, "\\x{code:02x}");
            } else {
                let _ = write!(escaped, "\\u{code:04x}");
            }
        } else {
            escaped.push(character);
        }
    }
    escaped
}

/// The fields of one combined access-log line, in nginx combined-log order:
/// `ip - "user" [time] "event" status bytes "referer" "user-agent"`.
struct Line {
    ip: String,
    user: String,
    event: String,
    status: StatusCode,
    bytes: u64,
    referer: String,
    user_agent: String,
}

impl Line {
    /// Renders the line with the supplied wall-clock timestamp (the event
    /// time). Pure and deterministic for testing.
    fn render(&self, timestamp: &str) -> String {
        format!(
            "{} - \"{}\" [{}] \"{}\" {} {} \"{}\" \"{}\"",
            escape_field(&self.ip),
            escape_field(&self.user),
            timestamp,
            escape_field(&self.event),
            self.status.as_u16(),
            self.bytes,
            escape_field(&self.referer),
            escape_field(&self.user_agent)
        )
    }

    fn emit(self) {
        emit(&self.render(&now_iso()));
    }
}

/// Logs a successful login: the session was minted, so the identity is known.
pub fn log_login_success(claims: &SessionClaims, ip: &str, user_agent: &str) {
    Line {
        ip: ip.to_string(),
        user: identity_label(Some(claims)),
        event: "LOGIN".to_string(),
        status: StatusCode::FOUND,
        bytes: 0,
        referer: UNKNOWN.to_string(),
        user_agent: user_agent.to_string(),
    }
    .emit();
}

/// The status a failed login is logged with: it mirrors the HTTP response the
/// callback would return for the same flow error.
fn login_status(error: &AuthFlowError) -> StatusCode {
    match error {
        AuthFlowError::Unauthorized => StatusCode::UNAUTHORIZED,
        AuthFlowError::Unavailable | AuthFlowError::ProviderUnavailable => {
            StatusCode::SERVICE_UNAVAILABLE
        }
    }
}

/// Logs a failed login: no identity is established, so the user is "-". The
/// status mirrors the HTTP response the callback would return.
pub fn log_login_failure(error: &AuthFlowError, ip: &str, user_agent: &str) {
    Line {
        ip: ip.to_string(),
        user: UNKNOWN.to_string(),
        event: "LOGIN".to_string(),
        status: login_status(error),
        bytes: 0,
        referer: UNKNOWN.to_string(),
        user_agent: user_agent.to_string(),
    }
    .emit();
}

/// Logs a single download outcome. The caller supplies the served status and
/// byte count (200 = file size, 206 = range length, otherwise 0).
pub fn log_download(
    ip: &str,
    user: &str,
    path: &str,
    status: StatusCode,
    bytes: u64,
    referer: &str,
    user_agent: &str,
) {
    let event = if path.is_empty() {
        "DOWNLOAD".to_string()
    } else {
        format!("DOWNLOAD {path}")
    };
    Line {
        ip: ip.to_string(),
        user: user.to_string(),
        event,
        status,
        bytes,
        referer: referer.to_string(),
        user_agent: user_agent.to_string(),
    }
    .emit();
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr};

    use axum::http::HeaderValue;

    use super::*;

    fn header_map(entries: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in entries {
            headers.insert(
                header::HeaderName::from_bytes(name.as_bytes()).expect("valid header name"),
                HeaderValue::from_str(value).expect("valid header value"),
            );
        }
        headers
    }

    fn peer(ip: [u8; 4]) -> ConnectInfo<SocketAddr> {
        ConnectInfo(SocketAddr::from((Ipv4Addr::from(ip), 50999)))
    }

    #[test]
    fn client_ip_is_unknown_without_peer_or_forwarded_info() {
        let headers = header_map(&[]);
        assert_eq!(client_ip(None, &headers, false), "-");
        assert_eq!(client_ip(None, &headers, true), "-");
    }

    #[test]
    fn client_ip_reads_the_peer_socket_by_default() {
        let headers = header_map(&[]);
        assert_eq!(
            client_ip(Some(&peer([127, 0, 0, 1])), &headers, false),
            "127.0.0.1"
        );
    }

    #[test]
    fn client_ip_ignores_forwarded_when_trust_is_off() {
        let headers = header_map(&[("x-forwarded-for", "203.0.113.7")]);
        assert_eq!(
            client_ip(Some(&peer([10, 0, 0, 2])), &headers, false),
            "10.0.0.2"
        );
    }

    #[test]
    fn client_ip_takes_the_left_most_forwarded_hop_when_trust_is_on() {
        let headers = header_map(&[("x-forwarded-for", "203.0.113.7, 10.0.0.1")]);
        assert_eq!(
            client_ip(Some(&peer([10, 0, 0, 2])), &headers, true),
            "203.0.113.7"
        );
    }

    #[test]
    fn client_ip_falls_back_to_peer_when_forwarded_is_absent_or_blank_under_trust() {
        let headers = header_map(&[]);
        assert_eq!(
            client_ip(Some(&peer([10, 0, 0, 2])), &headers, true),
            "10.0.0.2"
        );

        let blank = header_map(&[("x-forwarded-for", "   ")]);
        assert_eq!(
            client_ip(Some(&peer([10, 0, 0, 2])), &blank, true),
            "10.0.0.2"
        );
    }

    #[test]
    fn client_ip_rejects_a_hop_that_is_not_an_ip_under_trust() {
        // MDN's X-Forwarded-For guidance: spoofed values may not be actual
        // addresses, so only a hop that parses as an IP is logged.
        let garbage = header_map(&[("x-forwarded-for", "not-an-ip")]);
        assert_eq!(
            client_ip(Some(&peer([10, 0, 0, 2])), &garbage, true),
            "10.0.0.2"
        );

        // A hop carrying a port is not an X-Forwarded-For address either.
        let port = header_map(&[("x-forwarded-for", "203.0.113.7:8080")]);
        assert_eq!(
            client_ip(Some(&peer([10, 0, 0, 2])), &port, true),
            "10.0.0.2"
        );

        // Delimiter characters (OWASP: sanitize CR/LF and delimiters) are
        // rejected before the value is considered, so they never reach the
        // log through the IP field either.
        let delimiters = header_map(&[("x-forwarded-for", "1.2.3.4;5.6.7.8")]);
        assert_eq!(
            client_ip(Some(&peer([10, 0, 0, 2])), &delimiters, true),
            "10.0.0.2"
        );
    }

    #[test]
    fn client_ip_accepts_an_ipv6_hop_under_trust() {
        let headers = header_map(&[("x-forwarded-for", "2001:db8:85a3:8d3:1319:8a2e:370:7348")]);
        assert_eq!(
            client_ip(Some(&peer([10, 0, 0, 2])), &headers, true),
            "2001:db8:85a3:8d3:1319:8a2e:370:7348"
        );
    }

    #[test]
    fn user_agent_and_referer_default_to_dash() {
        let headers = header_map(&[]);
        assert_eq!(user_agent(&headers), "-");
        assert_eq!(referer(&headers), "-");

        let headers = header_map(&[
            ("user-agent", "curl/8.0"),
            ("referer", "https://files.example.com/"),
        ]);
        assert_eq!(user_agent(&headers), "curl/8.0");
        assert_eq!(referer(&headers), "https://files.example.com/");
    }

    #[test]
    fn identity_label_falls_back_to_subject_and_dash() {
        let claims = crate::auth::session_claims_for_tests();
        assert_eq!(identity_label(Some(&claims)), claims.display_name);

        let mut no_name = claims.clone();
        no_name.display_name = String::new();
        assert_eq!(identity_label(Some(&no_name)), no_name.subject);

        assert_eq!(identity_label(None), "-");
    }

    #[test]
    fn render_produces_the_combined_shape() {
        let line = Line {
            ip: "203.0.113.7".to_string(),
            user: "alice".to_string(),
            event: "DOWNLOAD docs/demo.txt".to_string(),
            status: StatusCode::PARTIAL_CONTENT,
            bytes: 512,
            referer: UNKNOWN.to_string(),
            user_agent: "curl/8.0".to_string(),
        };
        assert_eq!(
            line.render("2026-01-01T00:00:00Z"),
            "203.0.113.7 - \"alice\" [2026-01-01T00:00:00Z] \"DOWNLOAD docs/demo.txt\" 206 512 \"-\" \"curl/8.0\""
        );
    }

    #[test]
    fn a_user_label_with_spaces_cannot_shift_the_columns() {
        // The display name is free-form, so the user field is quoted like the
        // referer and user-agent: a space inside it stays inside the quoted
        // field and cannot shift every following column.
        let line = Line {
            ip: "203.0.113.7".to_string(),
            user: "Test User".to_string(),
            event: "DOWNLOAD docs/demo.txt".to_string(),
            status: StatusCode::OK,
            bytes: 5,
            referer: UNKNOWN.to_string(),
            user_agent: UNKNOWN.to_string(),
        };
        assert_eq!(
            line.render("2026-01-01T00:00:00Z"),
            "203.0.113.7 - \"Test User\" [2026-01-01T00:00:00Z] \"DOWNLOAD docs/demo.txt\" 200 5 \"-\" \"-\""
        );
    }

    #[test]
    fn c1_controls_and_unicode_separators_cannot_forge_log_lines() {
        let line = Line {
            ip: "127.0.0.1".to_string(),
            user: "a\u{0085}b\u{2028}c\u{2029}d".to_string(),
            event: "DOWNLOAD docs/demo.txt".to_string(),
            status: StatusCode::OK,
            bytes: 5,
            referer: UNKNOWN.to_string(),
            user_agent: UNKNOWN.to_string(),
        };
        let rendered = line.render("2026-01-01T00:00:00Z");
        assert_eq!(
            rendered,
            "127.0.0.1 - \"a\\x85b\\u2028c\\u2029d\" [2026-01-01T00:00:00Z] \"DOWNLOAD docs/demo.txt\" 200 5 \"-\" \"-\""
        );
    }

    #[test]
    fn control_characters_and_quotes_cannot_forge_log_lines() {
        let line = Line {
            ip: "127.0.0.1".to_string(),
            user: UNKNOWN.to_string(),
            event: "DOWNLOAD a\nb\"c\\d".to_string(),
            status: StatusCode::NOT_FOUND,
            bytes: 0,
            referer: UNKNOWN.to_string(),
            user_agent: "curl \"injected\"".to_string(),
        };
        let rendered = line.render("2026-01-01T00:00:00Z");
        // A newline would split the access log into two lines; it renders as
        // the escaped form instead, and quotes and backslashes stay inside
        // their quoted fields.
        assert!(
            !rendered.contains('\n'),
            "a control character forged a new log line: {rendered}"
        );
        assert!(
            rendered.contains("DOWNLOAD a\\x0ab\\\"c\\\\d"),
            "expected escaped control characters and quotes, got: {rendered}"
        );
        assert!(
            rendered.contains("curl \\\"injected\\\""),
            "expected an escaped user agent, got: {rendered}"
        );
    }

    #[test]
    fn login_failure_status_maps_unauthorized_and_provider() {
        let statuses = [
            (&AuthFlowError::Unauthorized, 401),
            (&AuthFlowError::Unavailable, 503),
            (&AuthFlowError::ProviderUnavailable, 503),
        ];
        for (error, expected) in statuses {
            let line = Line {
                ip: "203.0.113.7".to_string(),
                user: UNKNOWN.to_string(),
                event: "LOGIN".to_string(),
                status: login_status(error),
                bytes: 0,
                referer: UNKNOWN.to_string(),
                user_agent: "-".to_string(),
            };
            let rendered = line.render("2026-01-01T00:00:00Z");
            assert!(
                rendered.contains(&format!("\"LOGIN\" {expected} 0")),
                "expected LOGIN {expected}, got: {rendered}"
            );
        }
    }

    #[test]
    fn login_success_line_names_the_user_and_a_302() {
        let claims = crate::auth::session_claims_for_tests();
        let line = Line {
            ip: "203.0.113.7".to_string(),
            user: identity_label(Some(&claims)),
            event: "LOGIN".to_string(),
            status: StatusCode::FOUND,
            bytes: 0,
            referer: UNKNOWN.to_string(),
            user_agent: "curl/8.0".to_string(),
        };
        let rendered = line.render("2026-01-01T00:00:00Z");
        assert!(rendered.starts_with(&format!(
            "203.0.113.7 - \"{}\" [",
            identity_label(Some(&claims))
        )));
        assert!(rendered.contains("\"LOGIN\" 302 0"));
    }
}
