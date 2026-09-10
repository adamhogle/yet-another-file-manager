use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use std::fs;
use std::sync::OnceLock;
use tempfile::TempDir;
use tower::util::ServiceExt;

/// The listing must honor `showHidden: true`, sort deterministically (directories
/// first, then case-insensitive by name) and report `parentPath` for nested paths.
/// Runs in its own test binary so it gets its own process-global config: the
/// api_contract binary initializes a different temp root with `showHidden: false`
/// and the config OnceCell is first-call-wins. Every test here shares one fixture
/// whose config path comes from a process-global lock, so the first-call-wins rule
/// cannot split the tests across different roots.
static FIXTURE: OnceLock<TempDir> = OnceLock::new();

fn fixture_config_path(temp: &TempDir) -> std::path::PathBuf {
    temp.path().join("yafm.config.yaml")
}

fn fixture() -> &'static TempDir {
    FIXTURE.get_or_init(|| {
        let temp = TempDir::new().expect("temp dir");
        let shared_root = temp.path().join("share");

        fs::create_dir_all(&shared_root).expect("create shared root");
        fs::create_dir_all(shared_root.join("outer").join("inner")).expect("create nested dirs");
        fs::create_dir(shared_root.join("zeta_dir")).expect("create zeta dir");
        fs::write(shared_root.join("alpha.txt"), "a").expect("write alpha");
        fs::write(shared_root.join("Bravo.txt"), "bravo").expect("write bravo");
        fs::write(shared_root.join(".hidden.txt"), "hidden").expect("write hidden");
        fs::write(
            shared_root.join("outer").join("inner").join("leaf.txt"),
            "leaf",
        )
        .expect("write leaf");

        fs::write(
            fixture_config_path(&temp),
            format!("sharedRoot: {}\nshowHidden: true\n", shared_root.display()),
        )
        .expect("write test config");

        temp
    })
}

async fn initialize() -> &'static TempDir {
    let temp = fixture();
    backend::initialize_app_config(&fixture_config_path(temp)).expect("initialize app config");
    // The gate fails closed on uninitialized auth state; the test-only
    // Disabled state preserves this binary's path-safety coverage.
    backend::initialize_auth_disabled_for_tests();
    temp
}

#[tokio::test]
async fn hidden_entries_are_listed_when_show_hidden_is_true() {
    initialize().await;
    let app = backend::app_router();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/directory")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("directory response");
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    let json: Value = serde_json::from_slice(&bytes).expect("json body");

    // Nothing is filtered: all five fixture entries are listed, including the
    // dotfile that `showHidden: false` would suppress.
    assert_eq!(json["entries"].as_array().expect("entries").len(), 5);
    let hidden = json["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .find(|entry| entry["name"] == ".hidden.txt")
        .expect("hidden entry listed when showHidden is true");
    assert_eq!(hidden["kind"], "file");
}

#[tokio::test]
async fn listing_sorts_directories_first_then_case_insensitive_by_name() {
    initialize().await;
    let app = backend::app_router();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/directory")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("directory response");
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    let json: Value = serde_json::from_slice(&bytes).expect("json body");

    // Directories come first regardless of their name (zeta_dir sorts before every
    // file), then entries are ordered case-insensitively: alpha.txt precedes
    // Bravo.txt, which a byte-wise comparison would reverse.
    let names: Vec<&str> = json["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|entry| entry["name"].as_str().expect("entry name"))
        .collect();
    assert_eq!(
        names,
        vec!["outer", "zeta_dir", ".hidden.txt", "alpha.txt", "Bravo.txt"]
    );
    assert_eq!(json["entries"][0]["kind"], "directory");
    assert_eq!(json["entries"][1]["kind"], "directory");
    assert_eq!(json["entries"][3]["kind"], "file");
}

#[tokio::test]
async fn nested_listing_reports_parent_path_and_root_reports_none() {
    initialize().await;
    let app = backend::app_router();

    // ?p=outer/inner reports the parent of the nested path.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/directory?p=outer/inner")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("directory response");
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    let json: Value = serde_json::from_slice(&bytes).expect("json body");
    assert_eq!(json["currentPath"], "outer/inner");
    assert_eq!(json["parentPath"], "outer");
    assert_eq!(json["entries"][0]["name"], "leaf.txt");

    // A single component has no parent segment: parentPath is the empty string.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/directory?p=outer")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("directory response");
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    let json: Value = serde_json::from_slice(&bytes).expect("json body");
    assert_eq!(json["currentPath"], "outer");
    assert_eq!(json["parentPath"], "");

    // The root listing has no parent at all: parentPath is null.
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/directory")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("directory response");
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    let json: Value = serde_json::from_slice(&bytes).expect("json body");
    assert_eq!(json["currentPath"], "");
    assert!(json["parentPath"].is_null());
}
