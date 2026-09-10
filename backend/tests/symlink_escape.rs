use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use tempfile::TempDir;
use tower::util::ServiceExt;

const OUTSIDE_CONTENT: &[u8] = b"content-outside-the-shared-root";

fn write_test_config(config_path: &Path, shared_root: &Path) {
    let config = format!("sharedRoot: {}\nshowHidden: false\n", shared_root.display());
    fs::write(config_path, config).expect("write test config");
}

/// The listing and the download endpoint must refuse entries whose symlink target
/// escapes the shared root: canonicalize resolves the link and the containment check
/// rejects the resolved path. Runs in its own test binary so it gets its own
/// process-global config and its own temp root for the escape fixtures.
#[tokio::test]
async fn symlinked_entries_escaping_the_shared_root_are_not_served() {
    let temp = TempDir::new().expect("temp dir");
    let shared_root = temp.path().join("share");
    let config_path = temp.path().join("yafm.config.yaml");

    fs::create_dir_all(&shared_root).expect("create shared root");
    fs::write(shared_root.join("secret.txt"), "legit payload").expect("write file");

    // An escape fixture: a file and a directory outside the shared root, each
    // reachable through a symlink planted inside the root.
    let outside_file = temp.path().join("outside.txt");
    let outside_dir = temp.path().join("outside-dir");
    fs::write(&outside_file, OUTSIDE_CONTENT).expect("write outside file");
    fs::create_dir(&outside_dir).expect("create outside dir");
    fs::write(outside_dir.join("leak.txt"), OUTSIDE_CONTENT).expect("write leaked file");
    symlink("../outside.txt", shared_root.join("escape.txt")).expect("create escape symlink");
    symlink("../outside-dir", shared_root.join("escape_dir")).expect("create escape dir symlink");

    // A link whose target stays inside the root must keep working: the containment
    // check rejects escapes, not symlinks as such.
    symlink("secret.txt", shared_root.join("internal_link.txt")).expect("create internal symlink");

    write_test_config(&config_path, &shared_root);
    backend::initialize_app_config(&config_path).expect("initialize app config");
    // The gate fails closed on uninitialized auth state; the test-only
    // Disabled state preserves this binary's symlink-safety coverage.
    backend::initialize_auth_disabled_for_tests();
    let app = backend::app_router();

    // A symlink inside the root pointing outside must not serve the outside file:
    // the resolved target fails the containment check and the response must not
    // carry the outside content.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/download?p=escape.txt")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("escape download response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    assert!(
        bytes
            .windows(OUTSIDE_CONTENT.len())
            .all(|window| window != OUTSIDE_CONTENT),
        "the escaped file's content must not appear in the response body"
    );

    // The directory behind a symlink is likewise refused.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/download?p=escape_dir")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("escape dir download response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // The listing must not expose the escape entries either: per-entry containment
    // checks skip anything resolving outside the root, while an internal link is
    // listed like any other entry.
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
    let names: Vec<&str> = json["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .filter_map(|entry| entry["name"].as_str())
        .collect();
    assert_eq!(
        names,
        vec!["internal_link.txt", "secret.txt"],
        "entries escaping the root must be skipped by the listing"
    );

    // The containment check must not break normal downloads, including an internal
    // symlink and a real file.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/download?p=internal_link.txt")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("internal download response");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    assert_eq!(&bytes[..], b"legit payload");

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/download?p=secret.txt")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("download response");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    assert_eq!(&bytes[..], b"legit payload");
}
