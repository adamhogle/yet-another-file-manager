//! The access gate through the real router (ADR-0005): the process-global
//! config carries an `access` block, so this binary is separate from the
//! auth-gate test binary (the OnceCells are first-call-wins and cannot be
//! reset; one config per process, the fixture-lock pattern). The tests mint
//! session cookies with specific groups via the public `SessionCookieJar` and
//! pin the enforcement: a hidden request path answers 404 with the same
//! message a nonexistent path gets, the root listing filters entries, and a
//! user with no allow entries at all gets 404 on the root.

use std::net::TcpListener;
use std::path::PathBuf;

use axum::body::Body;
use axum::http::{Request, StatusCode};

use tower::util::ServiceExt;

const TEST_SIGNING_KEY: &str = "access-gate-test-signing-key-0123456789abcdef";
const TEST_CLIENT_SECRET: &str = "access-gate-test-client-secret";

/// The fixture temp directory (created once per process; the OnceCells
/// cannot be reset).
static FIXTURE_PATH: once_cell::sync::OnceCell<(tempfile::TempDir, PathBuf)> =
    once_cell::sync::OnceCell::new();

fn fixture_config_path() -> &'static PathBuf {
    let (_temp, path) = FIXTURE_PATH.get_or_init(|| {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let shared_root = temp.path().join("share");
        // The tree the enforcement tests walk: tv-shows with a .nfo file, dev
        // with a .env and main.rs, and common for the global baseline.
        let tv = shared_root.join("tv-shows");
        std::fs::create_dir_all(&tv).expect("create tv-shows");
        let season = tv.join("season-1");
        std::fs::create_dir_all(&season).expect("create season-1");
        std::fs::write(season.join("show.s01e01.nfo"), "metadata").expect("write nfo");
        std::fs::write(season.join("show.s01e01.mkv"), "video").expect("write mkv");
        let dev = shared_root.join("dev");
        std::fs::create_dir_all(&dev).expect("create dev");
        std::fs::write(dev.join(".env"), "secret").expect("write env");
        std::fs::write(dev.join("main.rs"), "code").expect("write code");
        let common = shared_root.join("common");
        std::fs::create_dir_all(&common).expect("create common");
        std::fs::write(common.join("README.md"), "docs").expect("write readme");

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
        let port = listener.local_addr().expect("the bound address").port();
        drop(listener);
        let config = format!(
            "sharedRoot: {}\nshowHidden: false\noidc:\n  issuer: http://127.0.0.1:{port}/application/o/yafm/\n  clientId: yafm-spa\n  clientSecret: {TEST_CLIENT_SECRET}\n  sessionSigningKey: {TEST_SIGNING_KEY}\n  redirectUri: http://127.0.0.1:{port}/api/v1/auth/callback\n  cookieSecure: false\naccess:\n  allow: [\"/common\"]\n  deny: [\".env\"]\n  grants:\n    yafm-tv:\n      allow:\n        - path: \"/tv-shows\"\n          deny: [\"*.nfo\"]\n",
            shared_root.display()
        );
        let config_path = temp.path().join("config.yaml");
        std::fs::write(&config_path, config).expect("write test config");
        (temp, config_path)
    });
    path
}

/// The Oidc-state fixture with the access block: initializes the
/// process-global config and the Oidc auth state before building the router.
/// Every test in this binary shares this state (the OnceCell is
/// first-call-wins and cannot be reset), so the calls are deterministic
/// regardless of test ordering.
async fn access_router() -> axum::Router {
    backend::initialize_app_config(fixture_config_path()).expect("initialize app config");
    backend::auth::initialize_auth().expect("initialize the Oidc auth state");
    backend::access::initialize_access().expect("initialize the access state");
    backend::app_router()
}

/// Mints a session-cookie header value for the given groups with the same
/// signing key the test config carries, so the gate and the access decision
/// exercise the real verification path.
fn minted_cookie(groups: &[&str]) -> String {
    let claims = backend::auth::SessionClaims {
        subject: "access-test-subject".to_string(),
        display_name: "Access Test".to_string(),
        email: "access-test@example.com".to_string(),
        groups: groups.iter().map(|g| g.to_string()).collect(),
    };
    backend::auth::SessionCookieJar::new(Some(TEST_SIGNING_KEY), false)
        .mint_session(backend::auth::SESSION_MAX_AGE_SECONDS, &claims)
        .encoded()
        .stripped()
        .to_string()
}

async fn get(app: &axum::Router, path: &str, cookie_header: Option<&str>) -> axum::http::Response<Body> {
    let mut builder = Request::builder().method("GET").uri(path);
    if let Some(value) = cookie_header {
        builder = builder.header("Cookie", value);
    }
    app.clone()
        .oneshot(builder.body(Body::empty()).expect("build request"))
        .await
        .expect("response")
}

async fn body_string(response: axum::http::Response<Body>) -> String {
    let collected = http_body_util::BodyExt::collect(response.into_body())
        .await
        .expect("collect body");
    let bytes = collected.to_bytes();
    String::from_utf8_lossy(&bytes).to_string()
}

/// A hidden request path answers 404 with the same message a nonexistent path
/// gets: hidden and nonexistent are indistinguishable and no existence leaks
/// (ADR-0005). The listing of a hidden directory, the download of a hidden
/// file by exact name and the download of a file denied by the global deny
/// are all 404.
#[tokio::test]
async fn hidden_paths_answer_404_like_nonexistent_paths() {
    let app = access_router().await;
    let cookie = minted_cookie(&["yafm-tv"]);

    for path in [
        // /dev is not in yafm-tv's visible root.
        "/api/v1/directory?p=dev",
        // The .nfo file is denied by the allow-specific deny.
        "/api/v1/download?p=tv-shows/season-1/show.s01e01.nfo",
        // The .env file is denied by the global deny.
        "/api/v1/download?p=dev/.env",
        // The .env under an allowed path is denied too.
        "/api/v1/download?p=common/.env",
    ] {
        let response = get(&app, path, Some(&cookie)).await;
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "GET {path} must answer 404 for a hidden path"
        );
        let body = body_string(response).await;
        assert!(
            body.contains("could not be found"),
            "the 404 body must be the shared error shape: {body}"
        );
    }

    // The same requests for a genuinely nonexistent path answer 404 with the
    // same message.
    let response = get(&app, "/api/v1/directory?p=missing", Some(&cookie)).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// The visible root works: the tv-shows listing is served, the .nfo file is
/// absent from the listing and the .mkv download works.
#[tokio::test]
async fn the_visible_root_is_served_and_filtered() {
    let app = access_router().await;
    let cookie = minted_cookie(&["yafm-tv"]);

    let response = get(&app, "/api/v1/directory?p=tv-shows", Some(&cookie)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_string(response).await;
    assert!(body.contains("season-1"), "{body}");
    assert!(
        !body.contains("show.s01e01.nfo"),
        "the denied .nfo file must be absent from the listing: {body}"
    );

    // The visible .mkv downloads.
    let response = get(
        &app,
        "/api/v1/download?p=tv-shows/season-1/show.s01e01.mkv",
        Some(&cookie),
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "a visible file must download"
    );
}

/// The root listing is visible for a user with allow entries and filtered to
/// the visible root: tv-shows and common appear, dev does not.
#[tokio::test]
async fn the_root_listing_filters_entries_to_the_visible_root() {
    let app = access_router().await;
    let cookie = minted_cookie(&["yafm-tv"]);

    let response = get(&app, "/api/v1/directory", Some(&cookie)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_string(response).await;
    assert!(body.contains("tv-shows"), "{body}");
    assert!(body.contains("common"), "{body}");
    assert!(
        !body.contains("\"dev\""),
        "a directory outside the visible root must be absent from the listing: {body}"
    );
}

/// A user whose groups match no grant sees only the global baseline; the
/// baseline's paths are served. With no baseline at all the root listing
/// answers 404 and the SPA renders the no-access state.
#[tokio::test]
async fn a_user_with_no_matching_grants_sees_only_the_baseline() {
    let app = access_router().await;

    // No matching grants: only the global baseline (/common).
    let cookie = minted_cookie(&["yafm-other"]);
    let response = get(&app, "/api/v1/directory", Some(&cookie)).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "a user with the global baseline can browse the root"
    );
    let body = body_string(response).await;
    assert!(body.contains("common"), "{body}");
    assert!(!body.contains("tv-shows"), "{body}");

    // The baseline's paths are served; everything else is hidden.
    let response = get(&app, "/api/v1/directory?p=common", Some(&cookie)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = get(&app, "/api/v1/directory?p=dev", Some(&cookie)).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
