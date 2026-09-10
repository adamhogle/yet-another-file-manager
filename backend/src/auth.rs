//! Authentication state, session cookies and the auth gate middleware for the
//! backend.

use std::time::{Duration as StdDuration, SystemTime, UNIX_EPOCH};

use axum::Json;
use axum::extract::{Query, Request};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use cookie::time::Duration;
use cookie::{Cookie, CookieJar, Key, SameSite};
use once_cell::sync::OnceCell;
use openidconnect::core::{CoreAuthenticationFlow, CoreClient};
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndSessionUrl, EndpointMaybeSet,
    EndpointNotSet, EndpointSet, IssuerUrl, Nonce, PkceCodeChallenge, PkceCodeVerifier,
    ProviderMetadataWithLogout, RedirectUrl, Scope, TokenResponse,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::sync::OnceCell as AsyncOnceCell;
use url::Url;

use crate::PublicErrorResponse;

/// Validated OIDC provider configuration from the config file's nested `oidc`
/// block. This is the single runtime shape. The parse-time shape lives in
/// lib.rs (`OidcConfigFile`); `load_app_config` validates the block into this
/// struct so the auth state initializer reads it from the process-global
/// config instead of re-reading the file.
///
/// Debug is written by hand with the secrets redacted: `client_secret` and
/// `session_signing_key` must never reach a log (the jar deliberately skips
/// `Debug` for the same reason).
#[derive(Clone)]
pub struct AuthOidcConfig {
    pub issuer: String,
    pub client_id: String,
    pub client_secret: String,
    /// The dedicated cookie-signing key. `None` means the jar generates an
    /// independent random key per process; the OIDC client secret must NOT
    /// fill this field — it alone would be enough to mint valid sessions.
    pub session_signing_key: Option<String>,
    pub redirect_uri: String,
    pub cookie_secure: bool,
}

impl std::fmt::Debug for AuthOidcConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthOidcConfig")
            .field("issuer", &self.issuer)
            .field("client_id", &self.client_id)
            .field("client_secret", &"[redacted]")
            .field(
                "session_signing_key",
                &self.session_signing_key.as_ref().map(|_| "[redacted]"),
            )
            .field("redirect_uri", &self.redirect_uri)
            .field("cookie_secure", &self.cookie_secure)
            .finish()
    }
}

/// The process-global authentication state. Production initializes it once at
/// startup from the validated config; the OnceCell mechanics (first successful
/// call wins, failed inits leave the cell empty) only matter for tests, where
/// each test process gets exactly one auth state (the fixture-lock pattern).
pub enum AuthState {
    /// Test-only state: only constructed by the debug-gated YAFM_DISABLE_AUTH
    /// startup path and `initialize_auth_disabled_for_tests`. Never reachable
    /// from production config (the `oidc` block is required and no env var
    /// disables auth in release builds).
    Disabled,
    Oidc(AuthOidcConfig),
}

static AUTH: OnceCell<AuthState> = OnceCell::new();

/// Initialize the process-global auth state from the already-validated
/// application config. Must be called after `initialize_app_config` (the
/// initializer reads the config through it); a missing `oidc` block is a
/// startup error because authentication is mandatory (ADR-0004). Idempotent
/// per the OnceCell mechanics: the first successful call wins.
pub fn initialize_auth() -> Result<(), String> {
    let config = crate::get_oidc_config().ok_or_else(|| {
        "Configuration oidc is required; the backend refuses to start without authentication."
            .to_string()
    })?;
    let _ = AUTH.set(AuthState::Oidc(config.clone()));
    Ok(())
}

/// Read access to the initialized auth state for the auth gate middleware.
/// None until initialized; the gate fails closed on None rather than serving
/// data without an auth check.
pub fn get_auth_state() -> Option<&'static AuthState> {
    AUTH.get()
}

/// Test-only: initialize the disabled auth state. Idempotent (first call
/// wins, matching the OnceCell mechanics); the Disabled variant is reachable
/// only through this helper and the debug-gated YAFM_DISABLE_AUTH startup
/// path, never from production config.
#[doc(hidden)]
pub fn initialize_auth_disabled_for_tests() {
    let _ = AUTH.set(AuthState::Disabled);
}

/// Name of the authenticated-session cookie minted by the callback endpoint
/// and verified by the gate on every request.
pub const SESSION_COOKIE_NAME: &str = "yafm_session";
/// Name of the short-lived login cookie set by the login endpoint (state,
/// nonce, PKCE verifier and the return-to path) and consumed by the callback.
pub const LOGIN_COOKIE_NAME: &str = "yafm_oidc_login";

/// The authenticated-session TTL: 12 hours. A named constant with no config
/// field — the session length is not per-deployment tunable (YAGNI); re-login
/// through authentik's SSO session is an instant redirect.
pub const SESSION_MAX_AGE_SECONDS: u64 = 12 * 60 * 60;

/// TTL of the short-lived login cookie (~10 minutes): it only has to survive
/// the authentik round trip and is consumed by the callback.
pub const LOGIN_MAX_AGE_SECONDS: u64 = 10 * 60;

/// The three auth endpoint paths, exempt from the gate so the login flow can
/// reach them. The routes are registered in `app_router()` (login, callback
/// and logout) alongside the data endpoints.
pub const LOGIN_PATH: &str = "/api/v1/auth/login";
pub const CALLBACK_PATH: &str = "/api/v1/auth/callback";
pub const LOGOUT_PATH: &str = "/api/v1/auth/logout";

/// Version tag inside the session-cookie payload (`<version>:<exp>`). Bumping
/// it invalidates every outstanding session so old cookies re-login instead of
/// failing to parse.
const SESSION_PAYLOAD_VERSION: &str = "v1";

/// Field separator of the login-cookie payload
/// (`<state>|<nonce>|<verifier>|<returnTo>`). The three OIDC fields are
/// alphanumeric/url-safe; the return-to path is the raw percent-encoded URI
/// string, and a conforming request target never contains a raw `|` (RFC 3986
/// requires percent-encoding; browsers percent-encode it). `splitn` keeps the
/// remainder intact, so return-to values round-trip exactly.
const LOGIN_FIELD_SEPARATOR: &str = "|";

/// Connect timeout for discovery, JWKS, and token-endpoint calls. A stalled
/// authentik connection must fail fast instead of hanging every login,
/// callback, and logout until the OS TCP stack gives up.
const OIDC_CONNECT_TIMEOUT_SECONDS: u64 = 10;
/// Total request timeout for one OIDC HTTP call (discovery, JWKS, and the
/// token exchange are all small payloads; 30s is generous).
const OIDC_REQUEST_TIMEOUT_SECONDS: u64 = 30;

static SESSION_COOKIE_JAR: OnceCell<SessionCookieJar> = OnceCell::new();

/// The process-global session-cookie jar, derived lazily from the initialized
/// auth state's signing key (or from a fresh random key when none is
/// configured). The config is fixed for the process lifetime, so the first
/// derivation wins and later calls reuse it (the OnceCell fixture-lock
/// pattern, matching the OIDC provider's discovery cache). Every
/// `AuthState::Oidc` value in one process must carry the same signing key for
/// the jar to stay consistent — production has exactly one config; tests
/// share one signing key.
fn session_cookie_jar(config: &AuthOidcConfig) -> &'static SessionCookieJar {
    SESSION_COOKIE_JAR.get_or_init(|| {
        SessionCookieJar::new(config.session_signing_key.as_deref(), config.cookie_secure)
    })
}

/// Mints and verifies the signed auth cookies (the 12-hour session and the
/// short-lived login cookie). Holds the signing key derived once from the
/// configured signing material, or an independent random key; deliberately
/// does NOT derive Debug — Debug output would leak the key.
pub struct SessionCookieJar {
    key: Key,
    secure: bool,
}

/// Data carried by the short-lived login cookie: the OIDC state, nonce and
/// PKCE code verifier are consumed by the callback to validate the flow, and
/// `return_to` is the same-origin relative path to redirect to after login.
#[derive(Debug, Clone)]
pub struct LoginCookieData {
    pub state: String,
    pub nonce: String,
    pub verifier: String,
    pub return_to: String,
}

impl SessionCookieJar {
    /// Signs cookies with the dedicated signing material, or generates an
    /// independent random key when none is configured. The OIDC client secret
    /// must NOT be the signing material: it alone would be enough to mint
    /// valid session cookies, and stateless signed cookies have no revocation
    /// path. `cookie::Key` requires a >=32-byte master key (HKDF-SHA256 inside
    /// the crate), so a configured secret is expanded with SHA-256 first; the
    /// derivation is deterministic, which keeps minted and verified cookies
    /// consistent across restarts.
    pub fn new(signing_key: Option<&str>, secure: bool) -> Self {
        let key = match signing_key {
            Some(material) => {
                let master_key = Sha256::digest(material.as_bytes());
                Key::derive_from(&master_key)
            }
            // An independent random key: the OIDC client secret alone can no
            // longer mint valid sessions. Sessions invalidate on restart —
            // the desired outcome after a key compromise.
            None => Key::generate(),
        };
        Self { key, secure }
    }

    /// Mints a signed session cookie whose payload records the payload format
    /// version and the unix-seconds expiration. The caller serializes the
    /// returned cookie with `Cookie::encoded()` into a Set-Cookie header.
    pub fn mint_session(&self, max_age_seconds: u64) -> Cookie<'static> {
        let expiration = unix_seconds_now() + max_age_seconds;
        let payload = format!("{SESSION_PAYLOAD_VERSION}:{expiration}");
        self.signed_cookie(SESSION_COOKIE_NAME, payload, max_age_seconds)
    }

    /// Verifies the session value carried by the `Cookie` request header:
    /// parses the header, verifies the signature with the derived key and
    /// checks the expiration. The error describes the problem so logs and
    /// tests stay actionable.
    pub fn verify_session(&self, header_value: &str) -> Result<(), String> {
        let cookie = extract_cookie(header_value, SESSION_COOKIE_NAME)
            .ok_or_else(|| "The session cookie is missing.".to_string())?;
        let value = self
            .verify_in_jar(cookie, SESSION_COOKIE_NAME)
            .ok_or_else(|| "The session cookie signature is invalid.".to_string())?;
        check_session_payload(&value)
    }

    /// A Max-Age=0 session cookie so the browser drops the session immediately.
    pub fn clear_session(&self) -> Cookie<'static> {
        clear_cookie(SESSION_COOKIE_NAME, self.secure)
    }

    /// Mints the short-lived signed login cookie carrying the OIDC state,
    /// nonce, PKCE code verifier and the return-to path.
    pub fn mint_login(&self, data: LoginCookieData) -> Cookie<'static> {
        let payload = [
            data.state.as_str(),
            data.nonce.as_str(),
            data.verifier.as_str(),
            data.return_to.as_str(),
        ]
        .join(LOGIN_FIELD_SEPARATOR);
        self.signed_cookie(LOGIN_COOKIE_NAME, payload, LOGIN_MAX_AGE_SECONDS)
    }

    /// Verifies the login value carried by the `Cookie` request header and
    /// returns the carried data (state, nonce, verifier, return-to) on
    /// success. The return-to value passes through unvalidated; the login and
    /// callback endpoints validate it before redirecting.
    pub fn verify_login(&self, header_value: &str) -> Result<LoginCookieData, String> {
        let cookie = extract_cookie(header_value, LOGIN_COOKIE_NAME)
            .ok_or_else(|| "The login cookie is missing.".to_string())?;
        let value = self
            .verify_in_jar(cookie, LOGIN_COOKIE_NAME)
            .ok_or_else(|| "The login cookie signature is invalid.".to_string())?;
        parse_login_payload(&value)
    }

    /// A Max-Age=0 login cookie so the browser drops the login state.
    pub fn clear_login(&self) -> Cookie<'static> {
        clear_cookie(LOGIN_COOKIE_NAME, self.secure)
    }

    /// Builds the cookie with the gate's attribute set and stores it through
    /// the signed jar; the stored cookie's value carries the signature and is
    /// ready for `Cookie::encoded()` serialization.
    fn signed_cookie(
        &self,
        name: &'static str,
        value: String,
        max_age_seconds: u64,
    ) -> Cookie<'static> {
        let cookie = Cookie::build((name, value))
            .path("/")
            .http_only(true)
            .same_site(SameSite::Lax)
            .secure(self.secure)
            .max_age(Duration::seconds(max_age_seconds as i64))
            .build();
        let mut jar = CookieJar::new();
        jar.signed_mut(&self.key).add(cookie);
        // The signed jar stores the value with the HMAC digest prepended;
        // read the signed cookie back through the plain jar so the Set-Cookie
        // value contains the signature the gate can verify later.
        jar.get(name)
            .expect("the cookie was just added to the jar")
            .clone()
    }

    /// Verifies a cookie parsed from a request header: the plain jar holds the
    /// incoming value and the signed jar only returns it when the signature
    /// verifies; the returned value is the authenticated payload.
    fn verify_in_jar(&self, cookie: Cookie<'static>, name: &str) -> Option<String> {
        let mut jar = CookieJar::new();
        jar.add(cookie);
        jar.signed(&self.key)
            .get(name)
            .map(|verified| verified.value().to_string())
    }
}

fn extract_cookie(header_value: &str, name: &str) -> Option<Cookie<'static>> {
    Cookie::split_parse_encoded(header_value)
        .filter_map(|result| result.ok())
        .find(|cookie| cookie.name() == name)
        .map(|cookie| cookie.into_owned())
}

fn clear_cookie(name: &'static str, secure: bool) -> Cookie<'static> {
    Cookie::build((name, ""))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(secure)
        .max_age(Duration::ZERO)
        .build()
}

fn check_session_payload(value: &str) -> Result<(), String> {
    let (version, expiration) = value
        .split_once(':')
        .ok_or_else(|| "The session cookie is not recognizable.".to_string())?;
    if version != SESSION_PAYLOAD_VERSION {
        return Err("The session cookie is not recognizable.".to_string());
    }
    let expiration: u64 = expiration
        .parse()
        .map_err(|_| "The session cookie is not recognizable.".to_string())?;
    if expiration <= unix_seconds_now() {
        return Err("The session cookie has expired.".to_string());
    }
    Ok(())
}

fn parse_login_payload(value: &str) -> Result<LoginCookieData, String> {
    let mut fields = value.splitn(4, LOGIN_FIELD_SEPARATOR);
    let state = fields.next().unwrap_or_default().to_string();
    let nonce = fields.next().unwrap_or_default().to_string();
    let verifier = fields.next().unwrap_or_default().to_string();
    let return_to = fields.next().unwrap_or_default().to_string();
    if state.is_empty() || nonce.is_empty() || verifier.is_empty() {
        return Err("The login cookie is not recognizable.".to_string());
    }
    Ok(LoginCookieData {
        state,
        nonce,
        verifier,
        return_to,
    })
}

fn unix_seconds_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock is after the unix epoch")
        .as_secs()
}

/// The auth gate: an all-or-nothing check on every route + fallback. The three
/// auth paths pass through; every other request needs a valid session (or the
/// test-only Disabled state), and the gate fails closed when the auth state is
/// missing. 401 JSON for /api/* requests (the generated client's error path
/// parses the message), 302 redirect for browser navigations (the SPA shell,
/// assets and downloads are plain navigations that cannot complete a
/// cross-origin fetch chain).
pub async fn auth_gate(req: Request, next: Next) -> Response {
    match gate_decision(
        get_auth_state(),
        req.uri().path(),
        req.headers()
            .get(header::COOKIE)
            .and_then(|value| value.to_str().ok()),
    ) {
        GateDecision::PassThrough => next.run(req).await,
        GateDecision::Unavailable => auth_unavailable_response(),
        GateDecision::Unauthorized => auth_required_response(),
        GateDecision::RedirectToLogin => {
            redirect_to_login(req.uri().path_and_query().map(|pq| pq.as_str()))
        }
    }
}

/// What the gate decided to do with a request.
enum GateDecision {
    /// Serve the request: a public path, the Disabled test state, or a valid session.
    PassThrough,
    /// Fail closed: the auth state is missing, never serve data.
    Unavailable,
    /// Unauthenticated /api/* request: JSON 401.
    Unauthorized,
    /// Unauthenticated browser navigation: 302 to the login endpoint.
    RedirectToLogin,
}

/// The gate's decision for a request, as a pure function of the auth state and
/// the request's path + Cookie header. The cookie verification reads the
/// process-global session-cookie jar derived lazily from the config's client
/// secret. A pure function so tests can classify every state without the
/// process-global auth state (one state per process — the fixture-lock
/// pattern): tests construct `AuthState` values directly instead of
/// initializing the global.
fn gate_decision(
    state: Option<&AuthState>,
    path: &str,
    cookie_header: Option<&str>,
) -> GateDecision {
    if is_public_path(path) {
        return GateDecision::PassThrough;
    }
    match state {
        // Fail closed: never serve data when auth state is missing.
        None => GateDecision::Unavailable,
        // Test-only state: preserves the pre-auth behavior of the existing
        // test binaries; never reachable from production config.
        Some(AuthState::Disabled) => GateDecision::PassThrough,
        Some(AuthState::Oidc(config)) => {
            let jar = session_cookie_jar(config);
            let session_valid = jar
                .verify_session(cookie_header.unwrap_or_default())
                .is_ok();
            if session_valid {
                GateDecision::PassThrough
            } else if path.starts_with("/api/") {
                GateDecision::Unauthorized
            } else {
                GateDecision::RedirectToLogin
            }
        }
    }
}

fn is_public_path(path: &str) -> bool {
    matches!(path, LOGIN_PATH | CALLBACK_PATH | LOGOUT_PATH)
}

/// The gate's fail-closed response: the auth state is missing, so the backend
/// cannot authenticate anything and must never serve data.
fn auth_unavailable_response() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(PublicErrorResponse {
            message: "Authentication is unavailable.".to_string(),
        }),
    )
        .into_response()
}

/// The gate's unauthenticated /api/* response: JSON in the existing
/// PublicErrorResponse shape so the generated client's error path parses the
/// message.
fn auth_required_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(PublicErrorResponse {
            message: "Authentication is required.".to_string(),
        }),
    )
        .into_response()
}

/// The provider-unavailable response for the auth flow endpoints: the OIDC
/// provider is unreachable or misconfigured (discovery or token calls fail).
/// A distinct message from the gate's fail-closed 503 — the authentication
/// STATE is present here, the upstream PROVIDER is the problem. Full sentence,
/// no host-path or upstream-detail leak.
fn auth_provider_unavailable_response() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(PublicErrorResponse {
            message: "The authentication provider is unavailable.".to_string(),
        }),
    )
        .into_response()
}

/// A 302 to the login endpoint carrying the original request path as the
/// `returnTo` query value. Only same-origin relative paths are forwarded (the
/// login endpoint validates the value again): a protocol-relative `//` target
/// — or any target containing a raw backslash, which browsers treat as a
/// solidus for special-scheme URLs — would redirect an authenticated browser
/// off-origin after login, so anything else falls back to the root.
fn redirect_to_login(return_to: Option<&str>) -> Response {
    let return_to = sanitize_return_to(return_to);
    let location = format!("{LOGIN_PATH}?returnTo={}", urlencoding::encode(&return_to));
    let mut headers = HeaderMap::new();
    // urlencoding::encode only emits ASCII (percent-encoding), so the header
    // value cannot fail to build; the static fallback is unreachable.
    if let Ok(value) = HeaderValue::from_str(&location) {
        headers.insert(header::LOCATION, value);
    }
    (StatusCode::FOUND, headers).into_response()
}

/// Only same-origin relative paths are forwarded as `returnTo`: the value must
/// start with a single `/`, not with `//` (protocol-relative redirect
/// protection), and must not contain a raw backslash. The WHATWG URL Standard
/// (implemented by every browser) treats `\` as a solidus in special-scheme
/// URLs, so a relative-looking `/\evil.example` resolves to
/// `https://evil.example/` exactly like `//evil.example`, and a conforming
/// same-origin request target never contains a raw backslash. Anything else
/// falls back to the root.
pub fn sanitize_return_to(return_to: Option<&str>) -> String {
    match return_to {
        Some(value)
            if value.starts_with('/') && !value.contains('\\') && !value.starts_with("//") =>
        {
            value.to_string()
        }
        _ => "/".to_string(),
    }
}

/// Test-only: mint a session-cookie value the auth gate accepts. The returned
/// string is the `Cookie` request-header value (name=value, percent-encoded) a
/// browser echoes back for the minted session; the gate accepts it when the
/// same signing key derives the jar's key, so the real signature/expiry
/// verification path is exercised.
#[doc(hidden)]
pub fn mint_session_cookie_for_tests(signing_key: Option<&str>, secure: bool) -> String {
    SessionCookieJar::new(signing_key, secure)
        .mint_session(SESSION_MAX_AGE_SECONDS)
        .encoded()
        .stripped()
        .to_string()
}

// ─── OIDC flow: lazy discovery + the login/callback/logout endpoints ────

/// The configured OIDC client type as openidconnect's typestate resolves it
/// after `CoreClient::from_provider_metadata` (the auth and token endpoints
/// come from discovery) plus `set_redirect_uri`. Named so the process-global
/// discovery cache can hold the client.
type DiscoveredClient = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

/// What a successful discovery produces, cached for the process lifetime: the
/// configured client (authorization + token endpoints, the discovered JWKS and
/// the configured redirect URI) and the end-session endpoint when the provider
/// publishes one. Discovery is lazy (first login/callback/logout use), so
/// startup makes no network call and container ordering cannot break startup;
/// a failed discovery leaves the cache empty so the next request retries (the
/// first SUCCESSFUL call wins).
struct DiscoveredProvider {
    client: DiscoveredClient,
    /// The HTTP client for the server-to-server token exchange, pooled across
    /// requests like the discovery fetch.
    http_client: openidconnect::reqwest::Client,
    end_session_url: Option<EndSessionUrl>,
}

static DISCOVERED_PROVIDER: AsyncOnceCell<DiscoveredProvider> = AsyncOnceCell::const_new();

/// The process-global discovered provider, fetched lazily on the first
/// login/callback/logout use. A failed discovery maps to the safe
/// provider-unavailable error: the details go to the log for operators, the
/// response never carries a raw upstream error.
async fn discovered_provider(
    config: &AuthOidcConfig,
) -> Result<&'static DiscoveredProvider, AuthFlowError> {
    DISCOVERED_PROVIDER
        .get_or_try_init(|| async { discover_provider(config).await })
        .await
        .map_err(|error| {
            tracing::warn!("OIDC provider discovery failed: {error}");
            AuthFlowError::ProviderUnavailable
        })
}

/// Fetches the provider metadata and JWKS from
/// `{issuer}.well-known/openid-configuration` and builds the configured
/// client. The discovery document's issuer must match the configured value
/// exactly (openidconnect compares them), so a misconfigured issuer fails
/// here instead of validating tokens against a different provider.
async fn discover_provider(config: &AuthOidcConfig) -> Result<DiscoveredProvider, String> {
    let issuer_url = IssuerUrl::new(discovery_issuer(config)).map_err(|error| error.to_string())?;
    let http_client = oidc_http_client()?;
    let metadata = ProviderMetadataWithLogout::discover_async(issuer_url, &http_client)
        .await
        .map_err(|error| format!("provider metadata fetch failed: {error}"))?;
    let end_session_url = metadata.additional_metadata().end_session_endpoint.clone();
    let client = CoreClient::<
        EndpointSet,
        EndpointNotSet,
        EndpointNotSet,
        EndpointNotSet,
        EndpointMaybeSet,
        EndpointMaybeSet,
    >::from_provider_metadata(
        metadata,
        ClientId::new(config.client_id.clone()),
        Some(ClientSecret::new(config.client_secret.clone())),
    )
    .set_redirect_uri(
        RedirectUrl::new(config.redirect_uri.clone()).map_err(|error| error.to_string())?,
    );
    Ok(DiscoveredProvider {
        client,
        http_client,
        end_session_url,
    })
}

/// The discovery URL is built by joining `.well-known/openid-configuration`
/// onto the issuer, and `Url::join` REPLACES the last path segment when the
/// issuer lacks a trailing slash — a configured issuer like
/// `https://host/application/o/<slug>` would send discovery to
/// `https://host/.well-known/...` instead of the provider's discovery path. A
/// missing trailing slash is normalized here so both configured forms work.
fn discovery_issuer(config: &AuthOidcConfig) -> String {
    let issuer = config.issuer.as_str();
    if issuer.ends_with('/') {
        issuer.to_string()
    } else {
        format!("{issuer}/")
    }
}

/// The HTTP client for discovery and the token exchange: rustls TLS (the
/// runtime image installs only ca-certificates, so an OpenSSL/curl stack would
/// fail to load) and redirects DISABLED — following redirects would open the
/// client up to SSRF (openidconnect's own recommendation).
fn oidc_http_client() -> Result<openidconnect::reqwest::Client, String> {
    openidconnect::reqwest::ClientBuilder::new()
        .redirect(openidconnect::reqwest::redirect::Policy::none())
        .connect_timeout(StdDuration::from_secs(OIDC_CONNECT_TIMEOUT_SECONDS))
        .timeout(StdDuration::from_secs(OIDC_REQUEST_TIMEOUT_SECONDS))
        .build()
        .map_err(|error| format!("the OIDC HTTP client failed to build: {error}"))
}

/// The failure modes of the login/callback/logout endpoints, mapped onto the
/// same safe PublicErrorResponse shapes the gate uses. The messages never leak
/// upstream details (provider URLs, OAuth error descriptions) or host paths.
#[derive(Debug)]
pub enum AuthFlowError {
    /// The auth state is missing (uninitialized) or the test-only Disabled
    /// state: the backend cannot authenticate anything. Same situation as the
    /// gate's fail-closed response.
    Unavailable,
    /// The OIDC provider is unreachable or misconfigured (a discovery or
    /// token-request failure): the browser can be sent through login again.
    ProviderUnavailable,
    /// The flow is not trustworthy: an OAuth `error` parameter, a
    /// missing/expired/forged login cookie, a state mismatch, or an ID token
    /// that fails validation.
    Unauthorized,
}

impl IntoResponse for AuthFlowError {
    fn into_response(self) -> Response {
        match self {
            AuthFlowError::Unavailable => auth_unavailable_response(),
            AuthFlowError::ProviderUnavailable => auth_provider_unavailable_response(),
            AuthFlowError::Unauthorized => auth_required_response(),
        }
    }
}

/// The login endpoint's query parameters: the gate forwards the original
/// request path as `returnTo`; a missing or non-same-origin value falls back
/// to the root (the sanitize check).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReturnToQuery {
    #[serde(default)]
    return_to: String,
}

/// The callback endpoint's query parameters: `code` + `state` on the happy
/// path, or the OAuth `error` parameter when the provider rejects the flow.
/// The `error_description` details are never read or leaked.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
}

/// `GET /api/v1/auth/login`: validates the return-to path, discovers the
/// provider lazily, generates state + nonce + PKCE verifier, sets the
/// short-lived signed login cookie and 302s the browser to the provider's
/// authorization endpoint (ADR-0004). A public path reachable even without an
/// initialized auth state, so it fails closed instead of panicking.
pub async fn login(Query(query): Query<ReturnToQuery>) -> Result<Response, AuthFlowError> {
    login_response(get_auth_state(), &query.return_to).await
}

/// `GET /api/v1/auth/callback`: validates the OAuth error parameter and the
/// signed login cookie, exchanges the code at the token endpoint
/// (server-to-server), validates the ID token (issuer, audience, expiration,
/// nonce) and issues the session cookie with a 302 to the return-to path.
pub async fn callback(
    Query(query): Query<CallbackQuery>,
    headers: HeaderMap,
) -> Result<Response, AuthFlowError> {
    callback_response(
        get_auth_state(),
        headers
            .get(header::COOKIE)
            .and_then(|value| value.to_str().ok()),
        &query,
    )
    .await
}

/// `GET /api/v1/auth/logout`: idempotent — clears the session cookie and 302s
/// to the provider's end-session endpoint. A public path reachable with no
/// initialized auth state, so it fails closed instead of panicking.
pub async fn logout() -> Result<Response, AuthFlowError> {
    logout_response(get_auth_state()).await
}

/// The login endpoint's logic: the uninitialized and test-only Disabled states
/// fail closed (the endpoints are public paths, reachable with no config);
/// otherwise the provider is discovered lazily, the authorization URL is built
/// with the generated state/nonce/PKCE challenge, and the signed login cookie
/// is set in the 302 response.
async fn login_response(
    state: Option<&AuthState>,
    return_to: &str,
) -> Result<Response, AuthFlowError> {
    let Some(AuthState::Oidc(config)) = state else {
        return Err(AuthFlowError::Unavailable);
    };
    let provider = discovered_provider(config).await?;
    let (authorization_url, login) =
        build_authorization_request(&provider.client, sanitize_return_to(Some(return_to)));
    let minted = session_cookie_jar(config).mint_login(login);
    Ok(redirect_with_cookies(
        authorization_url.as_str(),
        vec![minted],
    ))
}

/// The callback endpoint's logic: the OAuth error parameter and the login
/// cookie are validated before any provider contact; the code is exchanged
/// with the PKCE verifier from the login cookie, the ID token is validated
/// (issuer, audience, expiration, nonce) and the session is minted in the
/// same response that clears the login cookie and redirects to the
/// validated return-to path.
async fn callback_response(
    state: Option<&AuthState>,
    cookie_header: Option<&str>,
    query: &CallbackQuery,
) -> Result<Response, AuthFlowError> {
    let code = callback_code(query)?;
    let Some(AuthState::Oidc(config)) = state else {
        return Err(AuthFlowError::Unavailable);
    };
    let jar = session_cookie_jar(config);
    // The login cookie is the signed state store: missing, expired or forged
    // values reject the flow before any provider contact.
    let login = jar
        .verify_login(cookie_header.unwrap_or_default())
        .map_err(|_| AuthFlowError::Unauthorized)?;
    // The state query param must match the cookie's state (CSRF protection);
    // the code itself is single-use at the provider.
    if login.state != query.state.as_deref().unwrap_or_default() {
        return Err(AuthFlowError::Unauthorized);
    }
    let provider = discovered_provider(config).await?;
    let token_response = provider
        .client
        .exchange_code(AuthorizationCode::new(code.to_string()))
        .map_err(|_| AuthFlowError::ProviderUnavailable)?
        .set_pkce_verifier(PkceCodeVerifier::new(login.verifier.clone()))
        .request_async(&provider.http_client)
        .await
        .map_err(|_| AuthFlowError::ProviderUnavailable)?;
    let id_token = token_response
        .id_token()
        .ok_or(AuthFlowError::Unauthorized)?;
    // The verifier checks the signature against the discovered JWKS and the
    // issuer/audience/expiration claims; `claims` runs the nonce comparison
    // (replay protection). email_verified is deliberately NOT validated.
    let nonce = Nonce::new(login.nonce.clone());
    let verifier = provider.client.id_token_verifier();
    id_token
        .claims(&verifier, &nonce)
        .map_err(|_| AuthFlowError::Unauthorized)?;
    // Single-use flow complete: mint the session, clear the login cookie and
    // redirect to the return-to path (sanitized again — the cookie is signed,
    // but the redirect target is still only trusted after the same-origin
    // check).
    let minted = jar.mint_session(SESSION_MAX_AGE_SECONDS);
    let cleared = jar.clear_login();
    Ok(redirect_with_cookies(
        sanitize_return_to(Some(&login.return_to)).as_str(),
        vec![minted, cleared],
    ))
}

/// The callback's query-parameter validation, as a pure function so the
/// OAuth-error case is testable without the process-global auth state: the
/// code must be present — an OAuth `error` parameter (denied access, etc.) or
/// a missing code means the flow failed. The upstream details are never
/// leaked in the response.
fn callback_code(query: &CallbackQuery) -> Result<&str, AuthFlowError> {
    query.code.as_deref().ok_or(AuthFlowError::Unauthorized)
}

/// The logout endpoint's logic: idempotent — the session cookie is cleared
/// unconditionally and the browser is sent to the provider's end-session
/// endpoint (RP-initiated logout with no parameters: id_token_hint and
/// post_logout_redirect_uri are deliberately omitted, keeping it minimal —
/// the provider shows its own end-session page and the SSO session is
/// terminated either way). On a discovery failure the cookie is still cleared
/// and the redirect falls back to the root, so logout never strands the user.
async fn logout_response(state: Option<&AuthState>) -> Result<Response, AuthFlowError> {
    let Some(AuthState::Oidc(config)) = state else {
        return Err(AuthFlowError::Unavailable);
    };
    let cleared = session_cookie_jar(config).clear_session();
    let end_session = discovered_provider(config)
        .await
        .ok()
        .and_then(|provider| provider.end_session_url.clone());
    Ok(logout_redirect(end_session, cleared))
}

/// The logout redirect, as a pure function of the (already resolved)
/// end-session endpoint and the clear cookie, so the response shape is
/// testable without a provider.
fn logout_redirect(end_session: Option<EndSessionUrl>, cleared: Cookie<'static>) -> Response {
    match end_session {
        Some(end_session_url) => {
            redirect_with_cookies(end_session_url.url().as_str(), vec![cleared])
        }
        None => redirect_with_cookies("/", vec![cleared]),
    }
}

/// Builds the authorization URL and the login-cookie data from a discovered
/// provider: state + nonce + PKCE S256, the explicit scopes and the
/// redirect URI from the config. The `openid` scope is requested
/// automatically by openidconnect's `authorize_url`; `profile` and `email`
/// are added explicitly because a client requesting no scopes would be
/// treated as requesting all configured scopes per authentik's behavior
/// (entitlements etc. — noisy). The login cookie is the anti-CSRF/state
/// store: the callback verifies the state against it before the exchange.
fn build_authorization_request(
    client: &DiscoveredClient,
    return_to: String,
) -> (Url, LoginCookieData) {
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let (authorization_url, csrf_token, nonce) = client
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        .add_scope(Scope::new("profile".to_string()))
        .add_scope(Scope::new("email".to_string()))
        .set_pkce_challenge(pkce_challenge)
        .url();
    let login = LoginCookieData {
        state: csrf_token.secret().to_string(),
        nonce: nonce.secret().to_string(),
        verifier: pkce_verifier.secret().to_string(),
        return_to,
    };
    (authorization_url, login)
}

/// A 302 carrying a Location header and one Set-Cookie header per cookie.
/// Every value only emits ASCII (openidconnect percent-encodes the query;
/// `Cookie::encoded()` percent-encodes the value), so the header values
/// cannot fail to build; the fallback branch is unreachable.
fn redirect_with_cookies(location: &str, cookies: Vec<Cookie<'static>>) -> Response {
    let mut headers = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(location) {
        headers.insert(header::LOCATION, value);
    }
    for cookie in cookies {
        if let Ok(value) = HeaderValue::from_str(&cookie.encoded().to_string()) {
            // append, not insert: each Set-Cookie header must be its own header.
            headers.append(header::SET_COOKIE, value);
        }
    }
    (StatusCode::FOUND, headers).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::get;
    use openidconnect::core::{
        CoreJwsSigningAlgorithm, CoreResponseType, CoreSubjectIdentifierType,
    };
    use openidconnect::{
        AuthUrl, ClientId, ClientSecret, EmptyAdditionalProviderMetadata, JsonWebKeySetUrl,
        LogoutProviderMetadata, ProviderMetadataWithLogout, ResponseTypes, TokenUrl,
    };
    use tower::util::ServiceExt;

    /// The dedicated signing key of the locally-constructed cookie jars in
    /// the mint/verify tests; the jar derivation is deterministic from the
    /// material, so the same value mints and verifies.
    const TEST_SIGNING_KEY: &str = "auth-test-signing-key";

    /// Every test that runs the gate's Oidc branch shares this config's
    /// signing key: the process-global session-cookie jar is derived from the
    /// first config it sees, so the Oidc-state tests must use the same
    /// signing key for the jar to stay consistent regardless of test
    /// ordering.
    const GATE_TEST_CLIENT_SECRET: &str = "gate-test-client-secret";

    /// The issuer and redirect URI of the test configs and the
    /// manually-constructed provider metadata (the same shape discovery
    /// returns, so the login URL construction is testable without a live IdP).
    const TEST_ISSUER: &str = "https://authentik.example.com/application/o/yafm/";
    const TEST_REDIRECT_URI: &str = "https://files.example.com/api/v1/auth/callback";

    fn gate_test_oidc_config() -> AuthOidcConfig {
        AuthOidcConfig {
            issuer: TEST_ISSUER.to_string(),
            client_id: "test-client".to_string(),
            client_secret: GATE_TEST_CLIENT_SECRET.to_string(),
            session_signing_key: Some(TEST_SIGNING_KEY.to_string()),
            redirect_uri: TEST_REDIRECT_URI.to_string(),
            cookie_secure: false,
        }
    }

    /// A CoreClient built from manually-constructed provider metadata — the
    /// same shape discovery returns, so the login URL construction is
    /// testable without a live IdP. The full discovery-dependent flow is
    /// T8's mock-IdP test.
    fn manual_discovered_client() -> DiscoveredClient {
        let metadata = ProviderMetadataWithLogout::new(
            IssuerUrl::new(TEST_ISSUER.to_string()).expect("a valid issuer URL"),
            AuthUrl::new("https://authentik.example.com/application/o/yafm/authorize/".to_string())
                .expect("a valid auth URL"),
            JsonWebKeySetUrl::new(
                "https://authentik.example.com/application/o/yafm/jwks/".to_string(),
            )
            .expect("a valid JWKS URL"),
            vec![ResponseTypes::new(vec![CoreResponseType::Code])],
            vec![CoreSubjectIdentifierType::Public],
            vec![CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256],
            LogoutProviderMetadata {
                end_session_endpoint: None,
                additional_metadata: EmptyAdditionalProviderMetadata {},
            },
        )
        .set_token_endpoint(Some(
            TokenUrl::new("https://authentik.example.com/application/o/yafm/token/".to_string())
                .expect("a valid token URL"),
        ));
        CoreClient::<
            EndpointSet,
            EndpointNotSet,
            EndpointNotSet,
            EndpointNotSet,
            EndpointMaybeSet,
            EndpointMaybeSet,
        >::from_provider_metadata(
            metadata,
            ClientId::new("test-client".to_string()),
            Some(ClientSecret::new(GATE_TEST_CLIENT_SECRET.to_string())),
        )
        .set_redirect_uri(
            RedirectUrl::new(TEST_REDIRECT_URI.to_string()).expect("a valid redirect URL"),
        )
    }

    /// One auth state per process: the OnceCell is process-global, so the
    /// Disabled state is initialized once here and a second call must be a
    /// no-op (first-call-wins mechanics). Tests that need a different state
    /// run in their own test binary (fixture-lock pattern).
    #[test]
    fn initialize_auth_disabled_for_tests_is_idempotent() {
        initialize_auth_disabled_for_tests();
        assert!(matches!(get_auth_state(), Some(AuthState::Disabled)));

        initialize_auth_disabled_for_tests();
        assert!(matches!(get_auth_state(), Some(AuthState::Disabled)));
    }

    /// initialize_auth errors on the missing `oidc` block before touching the
    /// auth state, so this assertion holds regardless of the Disabled-state
    /// test above. It relies on the lib test binary's convention that no test
    /// initializes the process-global CONFIG with an oidc block (OnceCells
    /// cannot be reset, so such a test would break this one).
    #[test]
    fn initialize_auth_requires_the_oidc_block() {
        let error = initialize_auth().expect_err("a missing oidc block should fail");

        assert_eq!(
            error,
            "Configuration oidc is required; the backend refuses to start without authentication."
        );
    }

    /// The Debug impl is written by hand with the secrets redacted: one
    /// future `tracing::debug!` on config must not leak the client secret or
    /// the signing key into logs.
    #[test]
    fn the_auth_oidc_config_debug_output_redacts_the_secrets() {
        let config = AuthOidcConfig {
            issuer: TEST_ISSUER.to_string(),
            client_id: "test-client".to_string(),
            client_secret: "the-oidc-client-secret-value".to_string(),
            session_signing_key: Some("the-signing-key-value".to_string()),
            redirect_uri: TEST_REDIRECT_URI.to_string(),
            cookie_secure: false,
        };
        let debug_output = format!("{config:?}");

        assert!(
            !debug_output.contains("the-oidc-client-secret-value"),
            "the client secret must not appear in Debug output: {debug_output}"
        );
        assert!(
            !debug_output.contains("the-signing-key-value"),
            "the signing key must not appear in Debug output: {debug_output}"
        );
    }

    #[test]
    fn minted_session_cookies_round_trip_their_attributes_and_value() {
        let jar = SessionCookieJar::new(Some(TEST_SIGNING_KEY), false);
        let minted = jar.mint_session(SESSION_MAX_AGE_SECONDS);

        // The Set-Cookie value carries the attributes; Secure is omitted for a
        // plain-HTTP deployment (cookieSecure defaults to false).
        let set_cookie = minted.encoded().to_string();
        assert!(set_cookie.starts_with("yafm_session="));
        assert!(set_cookie.contains("HttpOnly"));
        assert!(set_cookie.contains("SameSite=Lax"));
        assert!(set_cookie.contains("Max-Age=43200"));
        assert!(set_cookie.contains("Path=/"));
        assert!(
            !set_cookie.contains("Secure"),
            "the Secure attribute must be omitted for plain HTTP"
        );

        // The name=value part a browser echoes back in the Cookie request
        // header verifies with the same signing key.
        let cookie_header = minted.encoded().stripped().to_string();
        jar.verify_session(&cookie_header)
            .expect("a freshly minted session must verify");
    }

    #[test]
    fn a_secure_session_cookie_carries_the_secure_attribute() {
        let jar = SessionCookieJar::new(Some(TEST_SIGNING_KEY), true);
        let minted = jar.mint_session(3600);

        let set_cookie = minted.encoded().to_string();
        assert!(set_cookie.contains("Secure"));
    }

    #[test]
    fn session_cookies_signed_with_a_different_signing_key_are_rejected() {
        let jar = SessionCookieJar::new(Some(TEST_SIGNING_KEY), false);
        let forged = SessionCookieJar::new(Some("a-different-signing-key"), false)
            .mint_session(3600)
            .encoded()
            .stripped()
            .to_string();

        let error = jar
            .verify_session(&forged)
            .expect_err("a cookie signed with a different signing key must be rejected");
        assert_eq!(error, "The session cookie signature is invalid.");
    }

    /// Two jars built without a configured signing key each generate their
    /// own random key: a cookie signed by one fails the other, proving the
    /// random fallback is per-process (and that no shared low-entropy
    /// material exists to forge across restarts).
    #[test]
    fn keyless_jars_generate_independent_keys_per_process() {
        let jar_a = SessionCookieJar::new(None, false);
        let jar_b = SessionCookieJar::new(None, false);
        let signed_by_a = jar_a.mint_session(3600).encoded().stripped().to_string();

        let error = jar_b
            .verify_session(&signed_by_a)
            .expect_err("a cookie signed by another keyless jar must be rejected");
        assert_eq!(error, "The session cookie signature is invalid.");
    }

    #[test]
    fn tampered_session_cookies_are_rejected() {
        let jar = SessionCookieJar::new(Some(TEST_SIGNING_KEY), false);
        let minted = jar.mint_session(3600).encoded().stripped().to_string();

        // Appending a character to the echoed value: the signature no longer
        // covers the tampered value.
        let tampered = format!("{minted}x");
        jar.verify_session(&tampered)
            .expect_err("a tampered value must be rejected");
    }

    #[test]
    fn expired_session_cookies_are_rejected() {
        let jar = SessionCookieJar::new(Some(TEST_SIGNING_KEY), false);
        // Max-Age=0 mints a payload that expires in the same second, so the
        // expiry check rejects it without needing a sleep.
        let expired = jar.mint_session(0).encoded().stripped().to_string();

        let error = jar
            .verify_session(&expired)
            .expect_err("an expired session must be rejected");
        assert_eq!(error, "The session cookie has expired.");
    }

    #[test]
    fn the_session_payload_check_rejects_unrecognizable_values() {
        let error = check_session_payload("").expect_err("an empty payload must be rejected");
        assert_eq!(error, "The session cookie is not recognizable.");

        let error = check_session_payload("v2:9999999999")
            .expect_err("an unknown payload version must be rejected");
        assert_eq!(error, "The session cookie is not recognizable.");

        let error = check_session_payload("v1:not-a-number")
            .expect_err("a non-numeric expiration must be rejected");
        assert_eq!(error, "The session cookie is not recognizable.");

        let error = check_session_payload("v1:0")
            .expect_err("a zero expiration must be rejected as expired");
        assert_eq!(error, "The session cookie has expired.");

        let far_future = unix_seconds_now() + 3600;
        check_session_payload(&format!("{SESSION_PAYLOAD_VERSION}:{far_future}"))
            .expect("a far-future expiration must be accepted");
    }

    #[test]
    fn garbage_cookie_headers_are_rejected_without_panicking() {
        let jar = SessionCookieJar::new(Some(TEST_SIGNING_KEY), false);
        // 44 chars of base64 followed by a payload: the digest length is
        // plausible but the signature does not verify.
        let plausible_digest = "AAAABBBBCCCCDDDDEEEEFFFFGGGGHHHHIIIIJJJJKKKK";
        let forged = format!("yafm_session={plausible_digest}~forged");
        for garbage in [
            "",
            "not-a-cookie",
            "yafm_session",
            "yafm_session=",
            "=value",
            "yafm_session=zzz",
            forged.as_str(),
            "other=1; yafm_session=zzz",
        ] {
            assert!(
                jar.verify_session(garbage).is_err(),
                "garbage {garbage:?} must be rejected"
            );
        }
    }

    #[test]
    fn login_cookies_round_trip_their_fields() {
        let jar = SessionCookieJar::new(Some(TEST_SIGNING_KEY), false);
        let minted = jar.mint_login(LoginCookieData {
            state: "state-token-abc".to_string(),
            nonce: "nonce-token-xyz".to_string(),
            verifier: "code-verifier-123".to_string(),
            return_to: "/docs%20and%20spaces".to_string(),
        });

        let cookie_header = minted.encoded().stripped().to_string();
        let verified = jar
            .verify_login(&cookie_header)
            .expect("a freshly minted login cookie must verify");

        assert_eq!(verified.state, "state-token-abc");
        assert_eq!(verified.nonce, "nonce-token-xyz");
        assert_eq!(verified.verifier, "code-verifier-123");
        assert_eq!(verified.return_to, "/docs%20and%20spaces");
    }

    #[test]
    fn login_cookies_round_trip_return_to_values_with_query_characters() {
        let jar = SessionCookieJar::new(Some(TEST_SIGNING_KEY), false);
        let return_to = "/deep/path?x=1&y=2";
        let minted = jar.mint_login(LoginCookieData {
            state: "s".to_string(),
            nonce: "n".to_string(),
            verifier: "v".to_string(),
            return_to: return_to.to_string(),
        });

        let verified = jar
            .verify_login(&minted.encoded().stripped().to_string())
            .expect("a return-to with '=' and '&' must round trip exactly");
        assert_eq!(verified.return_to, return_to);
    }

    #[test]
    fn missing_and_forged_login_cookies_are_rejected() {
        let jar = SessionCookieJar::new(Some(TEST_SIGNING_KEY), false);
        assert!(
            jar.verify_login("").is_err(),
            "a missing cookie is rejected"
        );

        let forged = SessionCookieJar::new(Some("a-different-signing-key"), false)
            .mint_login(LoginCookieData {
                state: "state".to_string(),
                nonce: "nonce".to_string(),
                verifier: "verifier".to_string(),
                return_to: "/".to_string(),
            })
            .encoded()
            .stripped()
            .to_string();
        assert!(
            jar.verify_login(&forged).is_err(),
            "a cookie signed with a different signing key must be rejected"
        );
    }

    #[test]
    fn cleared_cookies_expire_immediately() {
        let jar = SessionCookieJar::new(Some(TEST_SIGNING_KEY), false);
        let set_cookie = jar.clear_session().encoded().to_string();
        assert!(set_cookie.starts_with("yafm_session="));
        assert!(set_cookie.contains("Max-Age=0"));

        let login_set_cookie = jar.clear_login().encoded().to_string();
        assert!(login_set_cookie.starts_with("yafm_oidc_login="));
        assert!(login_set_cookie.contains("Max-Age=0"));
    }

    #[test]
    fn the_three_auth_paths_are_exempt_from_the_gate() {
        assert!(is_public_path(LOGIN_PATH));
        assert!(is_public_path(CALLBACK_PATH));
        assert!(is_public_path(LOGOUT_PATH));

        // Near misses must still be gated.
        assert!(!is_public_path("/api/v1/auth/logins"));
        assert!(!is_public_path("/api/v1/auth/logi"));
        assert!(!is_public_path("/api/v1/auth/login/extra"));
        assert!(!is_public_path("/api/v1/auth"));
        assert!(!is_public_path("/api/v1/auth/loginx"));
        assert!(!is_public_path("/api/v1/not-a-real-route"));
        assert!(!is_public_path("/api/v1/health"));
        assert!(!is_public_path("/"));
    }

    /// A minimal router for the gate's middleware-level behavior: the gate
    /// rejects before handlers run, so the handlers are trivial.
    fn gate_router() -> Router {
        Router::new()
            .route("/api/v1/ping", get(|| async { "ping" }))
            .fallback(get(|| async { "fallback" }))
            .layer(axum::middleware::from_fn(auth_gate))
    }

    /// The gate passes requests through for the Disabled state. All tests in
    /// this binary initialize the same Disabled state, so this call is
    /// deterministic regardless of test ordering (OnceCell first-call-wins);
    /// the uninitialized and Oidc states need their own process (T7's
    /// auth_gate.rs) because the OnceCells cannot be reset — the decision
    /// function tests below cover those states without the process global.
    #[tokio::test]
    async fn the_gate_passes_requests_through_when_auth_is_disabled() {
        initialize_auth_disabled_for_tests();

        let app = gate_router();
        for path in ["/api/v1/ping", "/some/asset", "/"] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .method("GET")
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("response");
            assert_eq!(response.status(), StatusCode::OK, "GET {path} passes");
        }
    }

    #[test]
    fn gate_decision_fails_closed_when_auth_state_is_missing() {
        // Fail closed for every non-public path: never serve data.
        for path in ["/api/v1/directory", "/api/v1/download", "/", "/missing.css"] {
            let decision = gate_decision(None, path, None);
            assert!(
                matches!(decision, GateDecision::Unavailable),
                "a missing auth state must fail closed for {path}"
            );
        }
    }

    #[test]
    fn gate_decision_passes_requests_through_when_auth_is_disabled() {
        for path in [
            "/api/v1/health",
            "/api/v1/directory?p=docs",
            "/some/deep/path",
        ] {
            let decision = gate_decision(Some(&AuthState::Disabled), path, None);
            assert!(matches!(decision, GateDecision::PassThrough), "GET {path}");
        }
        // Public paths are exempt regardless of the state.
        for path in [LOGIN_PATH, CALLBACK_PATH, LOGOUT_PATH] {
            let decision = gate_decision(Some(&AuthState::Disabled), path, None);
            assert!(
                matches!(decision, GateDecision::PassThrough),
                "{path} is public"
            );
        }
    }

    #[test]
    fn gate_decision_rejects_unauthenticated_api_requests() {
        let config = gate_test_oidc_config();
        for (path, cookie_header) in [
            ("/api/v1/health", None),
            ("/api/v1/directory?p=docs", None),
            ("/api/v1/download?p=docs/demo.txt", None),
            ("/api/v1/directory", Some("yafm_session=zzz")),
            ("/api/v1/directory", Some("yafm_session=not-a-signature")),
        ] {
            let decision =
                gate_decision(Some(&AuthState::Oidc(config.clone())), path, cookie_header);
            assert!(
                matches!(decision, GateDecision::Unauthorized),
                "unauthenticated GET {path} must be a JSON 401"
            );
        }
    }

    #[test]
    fn gate_decision_redirects_unauthenticated_browser_navigations() {
        let config = gate_test_oidc_config();
        for path in ["/", "/some/deep/path", "/missing-asset.css", "/favicon.svg"] {
            let decision = gate_decision(Some(&AuthState::Oidc(config.clone())), path, None);
            assert!(
                matches!(decision, GateDecision::RedirectToLogin),
                "unauthenticated GET {path} must redirect to login"
            );
        }
    }

    #[test]
    fn gate_decision_accepts_a_valid_minted_session() {
        let config = gate_test_oidc_config();
        let jar =
            SessionCookieJar::new(config.session_signing_key.as_deref(), config.cookie_secure);
        let cookie_header = jar
            .mint_session(SESSION_MAX_AGE_SECONDS)
            .encoded()
            .stripped()
            .to_string();

        for path in [
            "/",
            "/api/v1/health",
            "/api/v1/directory?p=docs",
            "/favicon.svg",
        ] {
            let decision = gate_decision(
                Some(&AuthState::Oidc(config.clone())),
                path,
                Some(&cookie_header),
            );
            assert!(
                matches!(decision, GateDecision::PassThrough),
                "a valid session must grant access to {path}"
            );
        }
    }

    #[test]
    fn gate_decision_redirects_to_a_same_origin_relative_return_to() {
        // The middleware builds the redirect from the original request target;
        // protocol-relative "//" values must fall back to the root. The WHATWG
        // URL Standard treats `\` as a solidus in special-scheme URLs, so the
        // backslash form is the same protocol-relative redirect and falls back
        // too.
        assert_eq!(sanitize_return_to(Some("/docs?p=1")), "/docs?p=1");
        assert_eq!(sanitize_return_to(Some("/")), "/");
        assert_eq!(sanitize_return_to(Some("//evil.example")), "/");
        assert_eq!(sanitize_return_to(Some("/\\evil.example")), "/");
        assert_eq!(sanitize_return_to(Some("/\\t")), "/");
        assert_eq!(sanitize_return_to(Some("/docs\\file")), "/");
        assert_eq!(sanitize_return_to(Some("https://evil.example")), "/");
        assert_eq!(sanitize_return_to(None), "/");
        assert_eq!(sanitize_return_to(Some("relative-path")), "/");
        // An empty value (a missing returnTo query param) falls back too.
        assert_eq!(sanitize_return_to(Some("")), "/");
    }

    #[test]
    fn the_login_redirect_targets_the_login_endpoint_with_an_encoded_return_to() {
        let response = redirect_to_login(Some("/docs?p=1"));
        assert_eq!(response.status(), StatusCode::FOUND);
        let location = response
            .headers()
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .expect("the redirect carries a Location header");
        assert_eq!(
            location, "/api/v1/auth/login?returnTo=%2Fdocs%3Fp%3D1",
            "the returnTo is percent-encoded for the query string"
        );

        // The fallback target is the root, with no host-path leak in the body
        // (the 302 carries no body at all).
        let response = redirect_to_login(Some("//evil.example"));
        assert_eq!(response.status(), StatusCode::FOUND);
        let location = response
            .headers()
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .expect("the redirect carries a location header");
        assert_eq!(location, "/api/v1/auth/login?returnTo=%2F");

        // The backslash form (`/\evil.example`, percent-encoded %2F%5C… in the
        // query string) is a protocol-relative redirect per the WHATWG URL
        // Standard and must fall back to the root like the `//` form.
        let response = redirect_to_login(Some("/\\evil.example"));
        assert_eq!(response.status(), StatusCode::FOUND);
        let location = response
            .headers()
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .expect("the redirect carries a location header");
        assert_eq!(location, "/api/v1/auth/login?returnTo=%2F");
    }

    #[test]
    fn the_gate_rejection_responses_use_the_public_error_shape() {
        let response = auth_unavailable_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

        let response = auth_required_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let response = auth_provider_unavailable_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    // ─── OIDC flow endpoint logic ───

    #[test]
    fn the_discovery_issuer_normalizes_a_missing_trailing_slash() {
        // Url::join replaces the last path segment when the issuer lacks a
        // trailing slash, which would send discovery to the wrong URL.
        let mut config = gate_test_oidc_config();
        config.issuer = "https://authentik.example.com/application/o/yafm".to_string();
        assert_eq!(
            discovery_issuer(&config),
            "https://authentik.example.com/application/o/yafm/"
        );

        config.issuer = "https://authentik.example.com/application/o/yafm/".to_string();
        assert_eq!(
            discovery_issuer(&config),
            "https://authentik.example.com/application/o/yafm/"
        );
    }

    /// The login URL construction from a manually-constructed client: state,
    /// nonce and PKCE challenge present, scopes exact, redirect URI from the
    /// config. The discovery-dependent parts are T8's mock-IdP test.
    #[test]
    fn the_login_builds_the_authorization_url_and_the_login_cookie() {
        let client = manual_discovered_client();
        let (authorization_url, login) =
            build_authorization_request(&client, "/docs?p=1".to_string());

        assert!(
            authorization_url
                .as_str()
                .starts_with("https://authentik.example.com/application/o/yafm/authorize/"),
            "the redirect targets the provider's authorization endpoint"
        );
        let query = authorization_url
            .query()
            .expect("the authorization URL carries a query string");
        let parameters: Vec<(&str, &str)> = query
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .collect();
        assert!(parameters.contains(&("response_type", "code")));
        assert!(parameters.iter().any(|(name, _)| *name == "state"));
        assert!(parameters.iter().any(|(name, _)| *name == "nonce"));
        assert!(parameters.iter().any(|(name, _)| *name == "code_challenge"));
        assert_eq!(
            parameters
                .iter()
                .find(|(name, _)| *name == "code_challenge_method"),
            Some(&("code_challenge_method", "S256")),
            "PKCE S256 is requested"
        );
        assert_eq!(
            parameters.iter().find(|(name, _)| *name == "scope"),
            Some(&("scope", "openid+profile+email")),
            "the scopes are exactly openid, profile and email"
        );
        assert!(
            parameters
                .iter()
                .any(|(name, value)| *name == "redirect_uri"
                    && *value == "https%3A%2F%2Ffiles.example.com%2Fapi%2Fv1%2Fauth%2Fcallback"),
            "the redirect URI comes from the config"
        );

        // The login cookie carries the generated state, nonce and PKCE
        // verifier and verifies with the signing key.
        let jar = SessionCookieJar::new(Some(TEST_SIGNING_KEY), false);
        let cookie_header = jar
            .mint_login(login.clone())
            .encoded()
            .stripped()
            .to_string();
        let verified = jar
            .verify_login(&cookie_header)
            .expect("a freshly minted login cookie must verify");
        assert_eq!(verified.state, login.state);
        assert_eq!(verified.nonce, login.nonce);
        assert_eq!(verified.verifier, login.verifier);
        assert_eq!(verified.return_to, "/docs?p=1");
    }

    #[test]
    fn the_login_response_carries_the_authorize_url_and_the_login_cookie() {
        let client = manual_discovered_client();
        let (authorization_url, login) = build_authorization_request(&client, "/".to_string());
        let jar = SessionCookieJar::new(Some(TEST_SIGNING_KEY), false);
        let response =
            redirect_with_cookies(authorization_url.as_str(), vec![jar.mint_login(login)]);

        assert_eq!(response.status(), StatusCode::FOUND);
        let location = response
            .headers()
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .expect("the redirect carries a Location header");
        assert_eq!(location, authorization_url.as_str());
        let set_cookie = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .collect::<Vec<_>>()
            .join("; ");
        assert!(set_cookie.starts_with("yafm_oidc_login="), "{set_cookie}");
        assert!(set_cookie.contains("HttpOnly"));
        assert!(set_cookie.contains("SameSite=Lax"));
        assert!(set_cookie.contains("Max-Age=600"), "{set_cookie}");
    }

    #[test]
    fn the_callback_rejects_an_oauth_error_parameter_without_leaking_details() {
        let error = callback_code(&CallbackQuery {
            code: None,
            state: None,
        })
        .expect_err("an OAuth error parameter means the flow failed");
        assert!(matches!(error, AuthFlowError::Unauthorized));

        let error = callback_code(&CallbackQuery {
            code: None,
            state: Some("state".to_string()),
        })
        .expect_err("a missing code means the flow failed");
        assert!(matches!(error, AuthFlowError::Unauthorized));

        assert!(
            callback_code(&CallbackQuery {
                code: Some("code".to_string()),
                state: None,
            })
            .is_ok()
        );
    }

    /// The login cookie is validated before any provider contact, so a
    /// missing cookie is rejected without discovery (no network, no panic).
    #[tokio::test]
    async fn the_callback_rejects_a_missing_login_cookie_before_contacting_the_provider() {
        let config = gate_test_oidc_config();
        let query = CallbackQuery {
            code: Some("code".to_string()),
            state: Some("state".to_string()),
        };

        let error = callback_response(Some(&AuthState::Oidc(config)), None, &query)
            .await
            .expect_err("a missing login cookie must be rejected");
        assert!(matches!(error, AuthFlowError::Unauthorized));
    }

    /// The state query param must match the login cookie's state (CSRF
    /// protection); the check runs before any provider contact.
    #[tokio::test]
    async fn the_callback_rejects_a_state_mismatch_before_contacting_the_provider() {
        let config = gate_test_oidc_config();
        let jar =
            SessionCookieJar::new(config.session_signing_key.as_deref(), config.cookie_secure);
        let cookie_header = jar
            .mint_login(LoginCookieData {
                state: "state-token-abc".to_string(),
                nonce: "nonce-token-xyz".to_string(),
                verifier: "code-verifier-123".to_string(),
                return_to: "/".to_string(),
            })
            .encoded()
            .stripped()
            .to_string();
        let query = CallbackQuery {
            code: Some("code".to_string()),
            state: Some("a-different-state".to_string()),
        };

        let error = callback_response(Some(&AuthState::Oidc(config)), Some(&cookie_header), &query)
            .await
            .expect_err("a state mismatch must be rejected");
        assert!(matches!(error, AuthFlowError::Unauthorized));
    }

    /// The Disabled state is test-only and cannot discover a provider: the
    /// auth endpoints are public paths reachable without a config, so they
    /// must fail closed instead of panicking.
    #[tokio::test]
    async fn the_login_fails_closed_when_auth_is_disabled() {
        initialize_auth_disabled_for_tests();

        let error = login_response(get_auth_state(), "/")
            .await
            .expect_err("the Disabled state cannot authenticate");
        assert!(matches!(error, AuthFlowError::Unavailable));
    }

    #[tokio::test]
    async fn the_callback_fails_closed_when_auth_is_disabled() {
        initialize_auth_disabled_for_tests();
        let query = CallbackQuery {
            code: Some("code".to_string()),
            state: Some("state".to_string()),
        };

        let error = callback_response(get_auth_state(), None, &query)
            .await
            .expect_err("the Disabled state cannot authenticate");
        assert!(matches!(error, AuthFlowError::Unavailable));
    }

    #[tokio::test]
    async fn the_logout_fails_closed_when_auth_is_disabled() {
        initialize_auth_disabled_for_tests();

        let error = logout_response(get_auth_state())
            .await
            .expect_err("the Disabled state cannot authenticate");
        assert!(matches!(error, AuthFlowError::Unavailable));
    }

    #[test]
    fn the_logout_redirect_clears_the_session_and_targets_the_end_session_endpoint() {
        let jar = SessionCookieJar::new(Some(TEST_SIGNING_KEY), false);
        let end_session = EndSessionUrl::new(
            "https://authentik.example.com/application/o/yafm/end-session/".to_string(),
        )
        .expect("a valid end-session URL");

        let response = logout_redirect(Some(end_session), jar.clear_session());
        assert_eq!(response.status(), StatusCode::FOUND);
        let location = response
            .headers()
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .expect("the redirect carries a Location header");
        assert_eq!(
            location,
            "https://authentik.example.com/application/o/yafm/end-session/"
        );
        let set_cookie = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .collect::<Vec<_>>()
            .join("; ");
        assert!(set_cookie.contains("yafm_session="), "{set_cookie}");
        assert!(set_cookie.contains("Max-Age=0"), "{set_cookie}");

        // A provider that publishes no end-session endpoint: logout still
        // clears the cookie and returns the browser to the root.
        let response = logout_redirect(None, jar.clear_session());
        assert_eq!(response.status(), StatusCode::FOUND);
        let location = response
            .headers()
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .expect("the redirect carries a Location header");
        assert_eq!(location, "/");
    }

    /// The discovery-failure branch of logout: the cookie is cleared and the
    /// redirect falls back to the root, so logout never strands the user. The
    /// discovery attempt runs against a guaranteed-closed loopback port
    /// (bound and immediately dropped), so it fails instantly.
    #[tokio::test]
    async fn logout_without_a_provider_clears_the_session_and_returns_to_the_root() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("binding a loopback port");
        let port = listener.local_addr().expect("the bound address").port();
        drop(listener);

        let config = AuthOidcConfig {
            issuer: format!("http://127.0.0.1:{port}/application/o/yafm/"),
            client_id: "test-client".to_string(),
            client_secret: GATE_TEST_CLIENT_SECRET.to_string(),
            session_signing_key: Some(TEST_SIGNING_KEY.to_string()),
            redirect_uri: format!("http://127.0.0.1:{port}/api/v1/auth/callback"),
            cookie_secure: false,
        };

        let response = logout_response(Some(&AuthState::Oidc(config)))
            .await
            .expect("logout must never strand the user on a discovery failure");
        assert_eq!(response.status(), StatusCode::FOUND);
        let location = response
            .headers()
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .expect("the redirect carries a Location header");
        assert_eq!(location, "/");
        let set_cookie = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .collect::<Vec<_>>()
            .join("; ");
        assert!(set_cookie.contains("yafm_session="), "{set_cookie}");
        assert!(set_cookie.contains("Max-Age=0"), "{set_cookie}");
    }
}
