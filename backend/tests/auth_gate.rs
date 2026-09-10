//! THE hard requirement: every endpoint must be secured by auth checks, and the
//! assertion must be explicit — the plan's ISC-1. Unauthenticated requests to
//! every path are rejected (401 JSON for `/api/*`, 302 to the login endpoint
//! for browser navigations), a valid signed session grants access to every
//! endpoint (ISC-4), forged and garbage cookies are rejected without a panic
//! (ISC-5), and the gate fails closed with 503 when the auth state is
//! uninitialized (ISC-9).
//!
//! Runs in its own test binary so it gets its own process-global auth state:
//! all Oidc-state tests share one fixture (one temp root, one config, one
//! client secret), and the fail-closed assertion runs in a spawned child
//! process because the OnceCell cannot be reset once a test initialized it
//! (tests run in parallel within a binary — the fixture-lock pattern).

use std::env;
use std::fs;
use std::net::TcpListener;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use http_body_util::BodyExt;
use serde_json::Value;
use tempfile::TempDir;
use tower::util::ServiceExt;

/// The client secret of this binary's test config (a required config field,
/// carried by the config even though the session-cookie jar no longer derives
/// from it).
const TEST_CLIENT_SECRET: &str = "gate-enumeration-client-secret";

/// The signing key of this binary's test config. The gate's process-global
/// session-cookie jar is derived from the config's `sessionSigningKey`, so a
/// minted session cookie verifies against the real router only when it is
/// minted with the SAME signing key (the deterministic derivation is what
/// makes the same-key mint valid — the real signature/expiry verification
/// path is exercised).
const TEST_SIGNING_KEY: &str = "auth-test-signing-key-0123456789abcdef";

/// The child-process mode of the fail-closed test: the auth state is a
/// process-global OnceCell that cannot be reset, and tests run in parallel
/// within a binary — the Oidc-state tests in this binary initialize it, so the
/// no-init assertions must run in a fresh process. The parent test spawns this
/// same test binary with the env var set; the child test executes them.
const FAIL_CLOSED_CHILD_ENV: &str = "YAFM_AUTH_GATE_FAIL_CLOSED_CHILD";

/// The exact name of the child-process test. libtest's `--exact` matches the
/// full test path; these tests are root-level, so the name is the function
/// name.
const FAIL_CLOSED_CHILD_TEST: &str = "the_uninitialized_gate_fails_closed_in_a_fresh_process";

/// Every path the gate must reject for unauthenticated requests. The three
/// auth endpoints are deliberately exempt (the login flow must reach them) and
/// have their own test; the fourth /api/* entry is the fallback route, which
/// the gate must cover too.
const API_PATHS: &[&str] = &[
    "/api/v1/health",
    "/api/v1/directory?p=docs",
    "/api/v1/download?p=docs/demo.txt",
    "/api/v1/nonexistent",
];

/// Every non-API path the gate must redirect for unauthenticated requests:
/// the SPA shell, an unknown asset, a known embedded asset and a deep path.
const BROWSER_PATHS: &[&str] = &["/", "/missing-asset.css", "/favicon.svg", "/foo/bar"];

fn all_gated_paths() -> Vec<&'static str> {
    let mut paths = Vec::with_capacity(API_PATHS.len() + BROWSER_PATHS.len());
    paths.extend_from_slice(API_PATHS);
    paths.extend_from_slice(BROWSER_PATHS);
    paths
}

/// The test binary's own temp root. Every test here shares one fixture whose
/// config carries the `oidc` block, so the process-global config OnceCell is
/// first-call-wins for exactly one root (the house pattern from the existing
/// test binaries).
static FIXTURE: OnceLock<TempDir> = OnceLock::new();

fn fixture_config_path(temp: &TempDir) -> std::path::PathBuf {
    temp.path().join("yafm.config.yaml")
}

fn fixture() -> &'static TempDir {
    FIXTURE.get_or_init(|| {
        let temp = TempDir::new().expect("temp dir");
        let shared_root = temp.path().join("share");
        fs::create_dir_all(shared_root.join("docs")).expect("create docs dir");
        fs::write(shared_root.join("docs").join("demo.txt"), "hello world")
            .expect("write demo file");
        fs::write(fixture_config_path(&temp), fixture_config(&shared_root))
            .expect("write test config");

        temp
    })
}

/// The test config with the `oidc` block. The issuer points at a
/// guaranteed-closed loopback port (bound and immediately dropped): the gate
/// needs no IdP interaction to reject unauthenticated requests, and the
/// login/logout handlers' lazy discovery attempt against a closed port fails
/// instantly (connection refused) instead of hanging — no live IdP in this
/// binary (the mock-IdP full-flow test is T8's).
fn fixture_config(shared_root: &Path) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
    let port = listener.local_addr().expect("the bound address").port();
    drop(listener);
    format!(
        "sharedRoot: {}\nshowHidden: false\noidc:\n  issuer: http://127.0.0.1:{port}/application/o/yafm/\n  clientId: yafm-spa\n  clientSecret: {TEST_CLIENT_SECRET}\n  sessionSigningKey: {TEST_SIGNING_KEY}\n  redirectUri: http://127.0.0.1:{port}/api/v1/auth/callback\n  cookieSecure: false\n",
        shared_root.display()
    )
}

/// The Oidc-state fixture: initializes the process-global config and the Oidc
/// auth state before building the router. Every Oidc-state test in this binary
/// shares this state (the OnceCell is first-call-wins and cannot be reset), so
/// the calls are deterministic regardless of test ordering.
async fn oidc_router() -> Router {
    let temp = fixture();
    backend::initialize_app_config(&fixture_config_path(temp)).expect("initialize app config");
    backend::auth::initialize_auth().expect("initialize the Oidc auth state from the test config");
    backend::app_router()
}

async fn get(app: &Router, path: &str, cookie_header: Option<&str>) -> Response {
    let mut builder = Request::builder().method("GET").uri(path);
    if let Some(value) = cookie_header {
        builder = builder.header("Cookie", value);
    }
    app.clone()
        .oneshot(builder.body(Body::empty()).expect("build request"))
        .await
        .expect("response")
}

/// A method-parameterized request helper alongside `get`. The gate is
/// method-agnostic (`gate_decision` never inspects the method — only state,
/// path and Cookie), so non-GET requests on data routes must be gated exactly
/// like GET: 401 for `/api/*`, 302 to the login endpoint for browser
/// navigations. `get` stays the default for the existing tests; this helper
/// is additive for pinning method-agnostic gating through the real router.
async fn request(app: &Router, method: &str, path: &str, cookie_header: Option<&str>) -> Response {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(value) = cookie_header {
        builder = builder.header("Cookie", value);
    }
    app.clone()
        .oneshot(builder.body(Body::empty()).expect("build request"))
        .await
        .expect("response")
}

/// The 401 rejection's shape: the PublicErrorResponse JSON with a non-empty
/// message and no host-path leak in the body.
async fn assert_api_rejection(app: &Router, path: &str, cookie_header: Option<&str>) {
    let temp = fixture();
    let response = get(app, path, cookie_header).await;

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "unauthenticated GET {path} must be a 401"
    );
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    let json: Value = serde_json::from_slice(&bytes).expect("the rejection carries JSON");
    let message = json["message"]
        .as_str()
        .expect("the PublicErrorResponse message");
    assert!(
        !message.trim().is_empty(),
        "the rejection message must be non-empty, got {message:?}"
    );
    let body = String::from_utf8(bytes.to_vec()).expect("body utf8");
    assert!(
        !body.contains(temp.path().to_str().expect("temp path utf8")),
        "the rejection body must not leak host paths: {body}"
    );
}

/// The 302 rejection's shape: a Location header pointing at the login endpoint
/// (a same-origin relative path, never an absolute external URL).
async fn assert_browser_rejection(app: &Router, path: &str, cookie_header: Option<&str>) {
    let response = get(app, path, cookie_header).await;

    assert_eq!(
        response.status(),
        StatusCode::FOUND,
        "unauthenticated GET {path} must redirect to login"
    );
    let location = response
        .headers()
        .get("location")
        .and_then(|value| value.to_str().ok())
        .expect("the redirect carries a Location header");
    assert!(
        location.starts_with("/api/v1/auth/login"),
        "the redirect must target the login endpoint, got {location}"
    );
    assert!(
        !location.starts_with("http://") && !location.starts_with("https://"),
        "the redirect target must not be an absolute external URL: {location}"
    );
}

/// THE hard requirement (ISC-1): every endpoint must reject unauthenticated
/// requests — 401 for /api/* paths, 302 to the login endpoint for non-API
/// paths — never a 200 with data. The three auth endpoints are exempt by
/// design (the login flow must reach them) and have their own test below.
#[tokio::test]
async fn unauthenticated_requests_to_every_endpoint_are_rejected() {
    let app = oidc_router().await;

    for path in all_gated_paths() {
        if path.starts_with("/api/") {
            assert_api_rejection(&app, path, None).await;
        } else {
            assert_browser_rejection(&app, path, None).await;
        }
    }
}

/// The gate is method-agnostic: `gate_decision` (auth.rs) never inspects the
/// request method — only state, path and Cookie — so OPTIONS/POST/HEAD on a
/// data route must be gated identically to GET, before any method dispatch
/// reaches a handler. The report probed these forms; this pins them through
/// the real router. Do NOT expect a 405: the gate rejects before method
/// handling, so `/api/*` is a 401 and `/` is a 302 to the login endpoint.
#[tokio::test]
async fn non_get_methods_are_gated_before_method_handling() {
    let app = oidc_router().await;

    for method in ["OPTIONS", "POST", "HEAD"] {
        // /api/* → JSON 401.
        let response = request(&app, method, "/api/v1/directory", None).await;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} /api/v1/directory must be gated as a 401"
        );

        // browser navigation → 302.
        let response = request(&app, method, "/", None).await;
        assert_eq!(
            response.status(),
            StatusCode::FOUND,
            "{method} / must redirect to login"
        );
        let location = response
            .headers()
            .get("location")
            .and_then(|value| value.to_str().ok())
            .expect("the redirect carries a Location header");
        assert!(
            location.starts_with("/api/v1/auth/login"),
            "{method} / must target the login endpoint, got {location}"
        );
    }
}

/// The report probed four public-path variants through the live binary and
/// found them all gated: `/api/v1/auth/%6Cogin` → 401, `/api/v1/auth/login/`
/// → 401, `/API/v1/auth/login` → 302, `//api/v1/auth/login` → 302. The gate
/// exempts ONLY the exact-match public paths; `req.uri().path()` is NOT
/// percent-decoded, so the `%6Cogin` form fails the exact-match exemption and
/// falls into the `/api/` branch (401), the trailing-slash form likewise, and
/// the case/double-slash forms are not `/api/` (case-sensitive prefix) so they
/// fall into the browser-navigation branch (302 to the login endpoint). T1
/// pinned these on the pure `is_public_path`/`gate_decision`; this proves the
/// real router routes them the same way. Do NOT add these variants to
/// `API_PATHS` (that const drives the whole-endpoint surface check) and do NOT
/// percent-decode the path.
#[tokio::test]
async fn path_variant_public_paths_are_gated_through_the_router() {
    let app = oidc_router().await;

    // Percent-encoded and trailing-slash are gated as /api/* (401 JSON).
    assert_api_rejection(&app, "/api/v1/auth/%6Cogin", None).await;
    assert_api_rejection(&app, "/api/v1/auth/login/", None).await;
    // Case and double-slash are NOT /api/ (case-sensitive) → 302 to login.
    assert_browser_rejection(&app, "/API/v1/auth/login", None).await;
    assert_browser_rejection(&app, "//api/v1/auth/login", None).await;
}

/// Forged and garbage session cookies must be rejected exactly like
/// unauthenticated requests (ISC-5): 401 for /api/*, 302 for browser
/// navigations, no panic. The tampered value starts as a real minted session
/// with one appended character, so the signature no longer covers it; the
/// forged value is signed with a different signing key via the same
/// doc-hidden helper the valid-session test uses.
#[tokio::test]
async fn forged_and_garbage_session_cookies_are_rejected() {
    let app = oidc_router().await;
    let minted = backend::auth::mint_session_cookie_for_tests(Some(TEST_SIGNING_KEY), false);
    let tampered = format!("{minted}x");
    let forged_with_other_key =
        backend::auth::mint_session_cookie_for_tests(Some("a-different-signing-key"), false);

    let cookie_cases: Vec<(&str, String)> = vec![
        ("/api/v1/health", "yafm_session=zzz".to_string()),
        ("/api/v1/health", "yafm_session=".to_string()),
        ("/api/v1/directory?p=docs", "not-a-cookie".to_string()),
        ("/api/v1/directory?p=docs", tampered.clone()),
        (
            "/api/v1/download?p=docs/demo.txt",
            forged_with_other_key.clone(),
        ),
        ("/", "yafm_session=zzz".to_string()),
        ("/missing-asset.css", tampered),
        ("/favicon.svg", forged_with_other_key),
    ];
    for (path, cookie) in cookie_cases {
        if path.starts_with("/api/") {
            assert_api_rejection(&app, path, Some(&cookie)).await;
        } else {
            assert_browser_rejection(&app, path, Some(&cookie)).await;
        }
    }
}

/// A valid minted session grants access to every endpoint (ISC-4). The cookie
/// is minted through the `#[doc(hidden)]` test helper with the SAME signing
/// key the test config carries — the gate's config-derived jar accepts it, so
/// the real signature/expiry verification path is exercised. The download
/// assertion needs a real file planted in the temp sharedRoot (the handler
/// 404s otherwise); the asset assertions rely on frontend/dist being present
/// at build time (rust-embed embeds it; CI builds the frontend before the
/// backend tests).
#[tokio::test]
async fn valid_session_grants_access_to_every_endpoint() {
    let cookie_header = backend::auth::mint_session_cookie_for_tests(Some(TEST_SIGNING_KEY), false);
    let app = oidc_router().await;

    let response = get(&app, "/api/v1/health", Some(&cookie_header)).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "a valid session must grant access to /api/v1/health"
    );
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    let json: Value = serde_json::from_slice(&bytes).expect("json body");
    assert_eq!(json["status"], "ok");
    assert_eq!(json["service"], "backend");

    let response = get(&app, "/api/v1/directory?p=docs", Some(&cookie_header)).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "a valid session must grant access to /api/v1/directory"
    );

    let response = get(
        &app,
        "/api/v1/download?p=docs/demo.txt",
        Some(&cookie_header),
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "a valid session must grant access to /api/v1/download"
    );
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    assert_eq!(&bytes[..], b"hello world", "the file's content is served");

    for path in BROWSER_PATHS {
        let response = get(&app, path, Some(&cookie_header)).await;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "a valid session must grant access to {path}"
        );
    }
}

/// The three auth endpoints are public paths: the gate exempts them, so
/// unauthenticated (and garbage-cookie) requests reach the handlers instead of
/// the gate's rejection. With the Oidc config in place but no live IdP, the
/// handlers fail closed with 503 and no panic: login and logout fail on the
/// lazy discovery (the issuer points at a closed loopback port, so the attempt
/// fails instantly) and the callback rejects the missing login cookie before
/// any provider contact. The full discovery-dependent flow is T8's mock-IdP
/// test.
#[tokio::test]
async fn the_auth_endpoints_reach_the_handlers_and_fail_closed_without_a_provider() {
    let app = oidc_router().await;

    let response = get(&app, "/api/v1/auth/login", None).await;
    assert_eq!(
        response.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "login must fail closed without a reachable provider"
    );
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    let json: Value = serde_json::from_slice(&bytes).expect("the failure carries JSON");
    assert_eq!(
        json["message"], "The authentication provider is unavailable.",
        "the failure carries the safe provider-unavailable message"
    );

    let response = get(&app, "/api/v1/auth/callback?code=probe&state=probe", None).await;
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "the callback must reject a missing login cookie before any provider contact"
    );

    let response = get(&app, "/api/v1/auth/logout", None).await;
    assert_eq!(
        response.status(),
        StatusCode::FOUND,
        "logout must never strand the user on a discovery failure"
    );
    let location = response
        .headers()
        .get("location")
        .and_then(|value| value.to_str().ok())
        .expect("the logout redirect carries a Location header");
    assert_eq!(location, "/", "the fallback redirect returns to the root");
    let set_cookie = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .collect::<Vec<_>>()
        .join("; ");
    assert!(
        set_cookie.contains("yafm_session=") && set_cookie.contains("Max-Age=0"),
        "logout still clears the session cookie: {set_cookie}"
    );
}

/// The fail-closed gate with an uninitialized auth state (ISC-9): the auth
/// state is a process-global OnceCell that cannot be reset, and the Oidc-state
/// tests in this binary initialize it — so the no-init assertions run in a
/// spawned child process of the same test binary (the fixture-lock pattern,
/// one auth state per process). NO config init, NO auth init, NO Disabled
/// state: the gate must fail closed with 503 for every enumerated path — never
/// 200 with data, never a panic, no redirect that would loop into a login that
/// can never succeed without auth state.
#[test]
fn the_gate_fails_closed_when_auth_state_is_uninitialized() {
    let output = Command::new(env::current_exe().expect("the test binary path"))
        .env(FAIL_CLOSED_CHILD_ENV, "1")
        .args(["--exact", FAIL_CLOSED_CHILD_TEST])
        .output()
        .expect("spawn the fail-closed child process");
    assert!(
        output.status.success(),
        "the uninitialized gate must fail closed for every path; stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// The child-process half of the fail-closed test: executed by
/// `the_gate_fails_closed_when_auth_state_is_uninitialized` with the child env
/// var set, in a fresh process where no test initialized the auth state. When
/// the binary runs normally (without the env var), the test is a no-op so the
/// regular suite stays green.
#[tokio::test]
async fn the_uninitialized_gate_fails_closed_in_a_fresh_process() {
    if env::var(FAIL_CLOSED_CHILD_ENV).as_deref() != Ok("1") {
        return;
    }

    let temp = fixture();
    let app = backend::app_router();
    for path in all_gated_paths() {
        let response = get(&app, path, None).await;
        assert_eq!(
            response.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "the uninitialized gate must fail closed for {path}"
        );
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        let json: Value =
            serde_json::from_slice(&bytes).expect("the fail-closed response carries JSON");
        let message = json["message"]
            .as_str()
            .expect("the PublicErrorResponse message");
        assert_eq!(
            message, "Authentication is unavailable.",
            "the fail-closed response carries the gate's unavailable message"
        );
        let body = String::from_utf8(bytes.to_vec()).expect("body utf8");
        assert!(
            !body.contains(temp.path().to_str().expect("temp path utf8")),
            "the fail-closed body must not leak host paths: {body}"
        );
    }
}
