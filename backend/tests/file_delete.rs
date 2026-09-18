//! The delete endpoint through the real router (ADR-0006): the process-global
//! config carries an `access` block with a delete-enabled grant, so this
//! binary is separate from the access-gate test binary (the OnceCells are
//! first-call-wins and cannot be reset; one config per process, the
//! fixture-lock pattern). The share tree is reset under the fixture lock at
//! the start of every test, so the mutating delete test leaves no residue for
//! the other tests. The tests pin the enforcement: a delete-enabled grant
//! deletes its own visible files, the global baseline is never deletable,
//! deny rules still win, hidden paths answer 404 like nonexistent paths, and
//! only regular files are deleted.

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use axum::body::Body;
use axum::http::{Request, StatusCode};

use tower::util::ServiceExt;

const TEST_SIGNING_KEY: &str = "file-delete-test-signing-key-0123456789abcdef";
const TEST_CLIENT_SECRET: &str = "file-delete-test-client-secret";

/// The fixture temp directory (created once per process; the OnceCells
/// cannot be reset). The share tree under it is reset per test.
static FIXTURE_PATH: once_cell::sync::OnceCell<(tempfile::TempDir, PathBuf)> =
    once_cell::sync::OnceCell::new();

/// The fixture lock: every test body holds it for its duration, so the
/// mutating delete test cannot race the read-only tests (the cargo test
/// harness runs the tests of one binary concurrently by default).
static FIXTURE_LOCK: Mutex<()> = Mutex::new(());

fn fixture_config_path() -> &'static PathBuf {
    let (_, path) = FIXTURE_PATH.get_or_init(|| {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let shared_root = temp.path().join("share");
        create_share_tree(&shared_root);

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
        let port = listener.local_addr().expect("the bound address").port();
        drop(listener);
        let config = format!(
            "sharedRoot: {}\nshowHidden: false\noidc:\n  issuer: http://127.0.0.1:{port}/application/o/yafm/\n  clientId: yafm-spa\n  clientSecret: {TEST_CLIENT_SECRET}\n  sessionSigningKey: {TEST_SIGNING_KEY}\n  redirectUri: http://127.0.0.1:{port}/api/v1/auth/callback\n  cookieSecure: false\naccess:\n  allow: [\"/common\"]\n  deny: [\".env\"]\n  grants:\n    yafm-devs:\n      delete: true\n      allow:\n        - path: \"/dev\"\n          deny: [\"dev/tmp/**\"]\n",
            shared_root.display()
        );
        let config_path = temp.path().join("config.yaml");
        std::fs::write(&config_path, config).expect("write test config");
        (temp, config_path)
    });
    path
}

fn fixture_shared_root() -> PathBuf {
    fixture_config_path()
        .parent()
        .expect("config parent")
        .join("share")
}

fn create_share_tree(shared_root: &Path) {
    let dev = shared_root.join("dev");
    std::fs::create_dir_all(&dev).expect("create dev");
    std::fs::write(dev.join("main.rs"), "code").expect("write main.rs");
    std::fs::write(dev.join(".env"), "secret").expect("write .env");
    let tmp_dir = dev.join("tmp");
    std::fs::create_dir_all(&tmp_dir).expect("create dev/tmp");
    std::fs::write(tmp_dir.join("scratch.txt"), "scratch").expect("write scratch.txt");
    let common = shared_root.join("common");
    std::fs::create_dir_all(&common).expect("create common");
    std::fs::write(common.join("README.md"), "docs").expect("write README.md");
    // A symlink whose target is a regular file inside the root: the
    // listing classifies it by the target's type, the delete endpoint
    // refuses it with 400.
    std::os::unix::fs::symlink(dev.join("main.rs"), dev.join("main.rs.link"))
        .expect("create symlink");
}

/// Recreates the share tree: the only cross-test residue is the file the
/// delete test removes, and the tests assert against the fixture tree the
/// config describes. The first call (before any test) may find no tree yet.
fn reset_share_tree() {
    let shared_root = fixture_shared_root();
    match std::fs::remove_dir_all(&shared_root) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("reset the share tree: {error}"),
    }
    create_share_tree(&shared_root);
}

/// The fixture-lock guard with a fresh share tree: every test takes it as its
/// first step and holds it for its duration, so the tests are serialized and
/// the reset cannot race a concurrent reader. Poisoning is collapsed into the
/// inner guard: a panicking test leaves the lock usable, the reset fixes the
/// tree, and the next test starts clean.
fn locked_fixture() -> MutexGuard<'static, ()> {
    let guard = FIXTURE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    reset_share_tree();
    guard
}

/// The Oidc-state fixture with the access block: initializes the
/// process-global config and the Oidc auth state before building the router.
/// The calls are first-call-wins and cannot be reset, so they are
/// deterministic regardless of test ordering.
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
        subject: "file-delete-test-subject".to_string(),
        display_name: "File Delete Test".to_string(),
        email: "file-delete-test@example.com".to_string(),
        groups: groups.iter().map(|g| g.to_string()).collect(),
    };
    backend::auth::SessionCookieJar::new(Some(TEST_SIGNING_KEY), false)
        .mint_session(backend::auth::SESSION_MAX_AGE_SECONDS, &claims)
        .encoded()
        .stripped()
        .to_string()
}

async fn request(
    app: &axum::Router,
    method: &str,
    path: &str,
    cookie_header: Option<&str>,
) -> axum::http::Response<Body> {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(value) = cookie_header {
        builder = builder.header("Cookie", value);
    }
    app.clone()
        .oneshot(builder.body(Body::empty()).expect("build request"))
        .await
        .expect("run request")
}

async fn body_string(response: axum::http::Response<Body>) -> String {
    let collected = http_body_util::BodyExt::collect(response.into_body())
        .await
        .expect("collect body");
    let bytes = collected.to_bytes();
    String::from_utf8_lossy(&bytes).to_string()
}

#[tokio::test]
async fn a_delete_enabled_grant_deletes_its_own_visible_files() {
    let _fixture = locked_fixture();
    let app = access_router().await;
    let cookie = minted_cookie(&["yafm-devs"]);

    let response = request(&app, "DELETE", "/api/v1/file?p=dev/main.rs", Some(&cookie)).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    // The file is really gone from the filesystem.
    assert!(!fixture_shared_root().join("dev/main.rs").exists());
}

#[tokio::test]
async fn a_visible_file_outside_the_delete_grant_answers_403() {
    let _fixture = locked_fixture();
    let app = access_router().await;
    let cookie = minted_cookie(&["yafm-devs"]);

    let response = request(
        &app,
        "DELETE",
        "/api/v1/file?p=common/README.md",
        Some(&cookie),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    // The file survives.
    assert!(fixture_shared_root().join("common/README.md").exists());
}

#[tokio::test]
async fn hidden_paths_answer_404_like_nonexistent_paths() {
    let _fixture = locked_fixture();
    let app = access_router().await;
    let cookie = minted_cookie(&["yafm-devs"]);

    // Hidden by the global deny (.env).
    let hidden = request(&app, "DELETE", "/api/v1/file?p=dev/.env", Some(&cookie)).await;
    assert_eq!(hidden.status(), StatusCode::NOT_FOUND);
    // Hidden by the allow-specific deny (dev/tmp/**).
    let hidden = request(
        &app,
        "DELETE",
        "/api/v1/file?p=dev/tmp/scratch.txt",
        Some(&cookie),
    )
    .await;
    assert_eq!(hidden.status(), StatusCode::NOT_FOUND);
    // Genuinely nonexistent.
    let missing = request(
        &app,
        "DELETE",
        "/api/v1/file?p=dev/no-such-file.txt",
        Some(&cookie),
    )
    .await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    // Hidden and nonexistent are indistinguishable: no existence leak.
    assert_eq!(body_string(hidden).await, body_string(missing).await);
}

#[tokio::test]
async fn directories_and_symlinks_are_refused_with_400() {
    let _fixture = locked_fixture();
    let app = access_router().await;
    let cookie = minted_cookie(&["yafm-devs"]);

    let directory = request(&app, "DELETE", "/api/v1/file?p=dev/tmp", Some(&cookie)).await;
    assert_eq!(directory.status(), StatusCode::BAD_REQUEST);
    assert!(fixture_shared_root().join("dev/tmp").is_dir());

    let symlink = request(
        &app,
        "DELETE",
        "/api/v1/file?p=dev/main.rs.link",
        Some(&cookie),
    )
    .await;
    assert_eq!(symlink.status(), StatusCode::BAD_REQUEST);
    // The link and its target survive.
    assert!(fixture_shared_root().join("dev/main.rs.link").exists());
    assert!(fixture_shared_root().join("dev/main.rs").exists());
}

#[tokio::test]
async fn a_session_without_delete_permission_answers_403() {
    let _fixture = locked_fixture();
    let app = access_router().await;
    // A user whose groups the config does not name: baseline only, no delete.
    let cookie = minted_cookie(&["yafm-other"]);

    let response = request(
        &app,
        "DELETE",
        "/api/v1/file?p=common/README.md",
        Some(&cookie),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_parent_dir_probe_answers_404_with_the_file_message() {
    let _fixture = locked_fixture();
    let app = access_router().await;
    let cookie = minted_cookie(&["yafm-devs"]);

    let response = request(&app, "DELETE", "/api/v1/file?p=..", Some(&cookie)).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    // The `..` probe surfaces the shared validator's ParentDir arm; the file
    // endpoint keeps the single hidden/nonexistent message shape the delete
    // contract pins.
    assert_eq!(
        body_string(response).await,
        "{\"message\":\"The requested file could not be found.\"}"
    );
}

#[tokio::test]
async fn the_listing_reports_can_delete_per_entry() {
    let _fixture = locked_fixture();
    let app = access_router().await;
    let cookie = minted_cookie(&["yafm-devs"]);

    let response = request(&app, "GET", "/api/v1/directory?p=dev", Some(&cookie)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_string(response).await;
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("the listing carries JSON");
    // The listing classifies an entry by its target's type (canonicalize
    // plus metadata), so the symlink-to-file row renders as a file whose
    // delete answers 400, the accepted edge case ADR-0006 documents. The
    // non-deletable value here comes from the tmp directory, never from
    // the symlink.
    let entry = |name: &str| {
        parsed["entries"]
            .as_array()
            .expect("the listing carries entries")
            .iter()
            .find(|item| item["name"] == name)
            .unwrap_or_else(|| panic!("the listing is missing {name}: {body}"))
            .clone()
    };
    assert_eq!(entry("main.rs")["kind"], "file");
    assert_eq!(entry("main.rs")["canDelete"], true);
    assert_eq!(entry("main.rs.link")["kind"], "file");
    assert_eq!(entry("main.rs.link")["canDelete"], true);
    assert_eq!(entry("tmp")["kind"], "directory");
    assert_eq!(entry("tmp")["canDelete"], false);
}
