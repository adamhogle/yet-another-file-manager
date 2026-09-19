//! Nginx-style access logging for logins and file downloads.
//!
//! Emits one combined-log-style line per login attempt (success or failure)
//! and per download outcome into the existing tracing stream under the
//! `yafm::access` target. The client IP resolution honors an opt-in
//! `trustProxy` config flag: when off (the default) the peer socket IP is
//! logged and a spoofed `X-Forwarded-For` is ignored; when on, the left-most
//! `X-Forwarded-For` hop (the client behind a trusted reverse proxy) is
//! logged, falling back to the peer IP when the header is absent.

use std::net::SocketAddr;

use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, StatusCode, header};
use chrono::Utc;

use crate::auth::{AuthFlowError, SessionClaims};

/// The tracing target access lines are emitted under, so operators can filter
/// or route them independently of application logs.
pub const ACCESS_LOG_TARGET: &str = "yafm::access";

const UNKNOWN: &str = "-";

/// Resolves the client IP to log. When `trust_proxy` is true, the left-most
/// `X-Forwarded-For` hop (the client behind a trusted reverse proxy) is used;
/// otherwise (or when the header is absent or blank) the peer socket IP. Both
/// absent -> "-", so nothing spoofed ever reaches the log unless the operator
/// enabled the trust.
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
        let ip = first.trim();
        if !ip.is_empty() {
            return ip.to_string();
        }
    }
    match connect_info {
        Some(connect_info) => connect_info.0.ip().to_string(),
        None => UNKNOWN.to_string(),
    }
}

/// The `User-Agent` value as sent, or "-" when absent.
pub fn user_agent(headers: &HeaderMap) -> String {
    headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or(UNKNOWN)
        .to_string()
}

/// The referer value as sent, or "-" when absent.
pub fn referer(headers: &HeaderMap) -> String {
    headers
        .get(header::REFERER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or(UNKNOWN)
        .to_string()
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

/// The fields of one combined access-log line, in nginx combined-log order:
/// `ip - user [time] "event" status bytes "referer" "user-agent"`.
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
            "{} - {} [{}] \"{}\" {} {} \"{}\" \"{}\"",
            self.ip,
            self.user,
            timestamp,
            self.event,
            self.status.as_u16(),
            self.bytes,
            self.referer,
            self.user_agent
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

/// Logs a failed login: no identity is established, so the user is "-". The
/// status mirrors the HTTP response the callback would return.
pub fn log_login_failure(error: &AuthFlowError, ip: &str, user_agent: &str) {
    let status = match error {
        AuthFlowError::Unauthorized => StatusCode::UNAUTHORIZED,
        AuthFlowError::Unavailable | AuthFlowError::ProviderUnavailable => {
            StatusCode::SERVICE_UNAVAILABLE
        }
    };
    Line {
        ip: ip.to_string(),
        user: UNKNOWN.to_string(),
        event: "LOGIN".to_string(),
        status,
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
            "203.0.113.7 - alice [2026-01-01T00:00:00Z] \"DOWNLOAD docs/demo.txt\" 206 512 \"-\" \"curl/8.0\""
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
                status: match error {
                    AuthFlowError::Unauthorized => StatusCode::UNAUTHORIZED,
                    _ => StatusCode::SERVICE_UNAVAILABLE,
                },
                bytes: 0,
                referer: UNKNOWN.to_string(),
                user_agent: "-".to_string(),
            };
            assert!(
                line.render("2026-01-01T00:00:00Z")
                    .contains(&format!("\"LOGIN\" {expected} 0")),
                "expected LOGIN {expected}, got: {}",
                line.render("2026-01-01T00:00:00Z")
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
            "203.0.113.7 - {} [",
            identity_label(Some(&claims))
        )));
        assert!(rendered.contains("\"LOGIN\" 302 0"));
    }
}
