use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use std::fs;
use std::path::Path;
use tempfile::TempDir;
use tower::util::ServiceExt;

fn write_test_config(config_path: &Path, shared_root: &Path) {
    let config = format!("sharedRoot: {}\nshowHidden: false\n", shared_root.display());
    fs::write(config_path, config).expect("write test config");
}

#[tokio::test]
async fn directory_endpoint_matches_contract_shape_and_status_codes() {
    let temp = TempDir::new().expect("temp dir");
    let shared_root = temp.path().join("share");
    let docs_dir = shared_root.join("docs");
    let config_path = temp.path().join("yafm.config.yaml");

    fs::create_dir_all(&docs_dir).expect("create docs dir");
    fs::write(docs_dir.join("demo.txt"), "hello").expect("write demo file");
    // showHidden defaults to false: the dotfile must be filtered from the listing.
    fs::write(docs_dir.join(".dotfile.txt"), "hidden").expect("write dotfile");
    write_test_config(&config_path, &shared_root);

    backend::initialize_app_config(&config_path).expect("initialize app config");
    // The gate fails closed on uninitialized auth state; the test-only Disabled
    // state preserves this test's purpose (path safety, contract shapes).
    backend::initialize_auth_disabled_for_tests();

    let app = backend::app_router();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/directory?p=docs")
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

    assert_eq!(json["currentPath"], "docs");
    assert_eq!(json["parentPath"], "");
    assert!(json["entries"].is_array());
    assert_eq!(
        json["entries"].as_array().expect("entries").len(),
        1,
        "the dotfile must be filtered when showHidden is false"
    );

    let first_entry = &json["entries"][0];
    assert_eq!(first_entry["name"], "demo.txt");
    assert_eq!(first_entry["kind"], "file");
    assert!(first_entry.get("sizeBytes").is_some());
    assert!(first_entry.get("modifiedAt").is_some());

    let invalid_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/directory?p=../secret")
                .method("GET")
                .body(Body::empty())
                .expect("invalid request"),
        )
        .await
        .expect("invalid response");

    assert_eq!(invalid_response.status(), StatusCode::NOT_FOUND);

    // Directory: double-encoded traversal (%252F is outer-decoded to %2F by the
    // query extractor, then percent_decode_relative_path decodes it to a literal
    // slash, yielding an absolute path that is rejected as 400).
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/directory?p=%252Fetc%252Fpasswd")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("directory response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // Download: valid file at root level
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/download?p=docs/demo.txt")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("download response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("application/octet-stream")
    );
    assert_eq!(
        response
            .headers()
            .get("content-disposition")
            .and_then(|v| v.to_str().ok()),
        Some("attachment; filename=\"demo.txt\"")
    );
    assert_eq!(
        response
            .headers()
            .get("x-content-type-options")
            .and_then(|v| v.to_str().ok()),
        Some("nosniff")
    );

    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    assert_eq!(&bytes[..], b"hello");

    // Download: empty path returns 400
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/download")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("download response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // Download: path traversal returns 404
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/download?p=../secret")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("download response");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // Download: double-encoded traversal (%252F outer-decodes to %2F then to a
    // literal slash; the resulting absolute path is rejected as 400).
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/download?p=%252Fetc%252Fpasswd")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("download response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // Download: absolute path returns 400
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/download?p=/etc/passwd")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("download response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // Download: non-existent file returns 404
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/download?p=missing.txt")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("download response");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // Download: directory target returns 404
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/download?p=docs")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("download response");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn embedded_frontend_serves_index_and_preserves_unknown_api_404() {
    // The gate fails closed on uninitialized auth state; the test-only
    // Disabled state preserves this test's contract-shape assertions.
    backend::initialize_auth_disabled_for_tests();

    let app = backend::app_router();

    let root_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/")
                .method("GET")
                .body(Body::empty())
                .expect("root request"),
        )
        .await
        .expect("root response");

    assert_eq!(root_response.status(), StatusCode::OK);
    assert_eq!(
        root_response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("text/html")
    );

    let root_bytes = root_response
        .into_body()
        .collect()
        .await
        .expect("collect root body")
        .to_bytes();
    let root_html = String::from_utf8(root_bytes.to_vec()).expect("root html utf8");
    assert!(root_html.contains("<div id=\"app\"></div>"));

    // An unknown non-API asset falls back to the embedded index.html so the SPA
    // router can render its own view for it.
    let unknown_asset_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/missing-asset.css")
                .method("GET")
                .body(Body::empty())
                .expect("unknown asset request"),
        )
        .await
        .expect("unknown asset response");

    assert_eq!(unknown_asset_response.status(), StatusCode::OK);
    assert_eq!(
        unknown_asset_response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("text/html")
    );

    let unknown_asset_bytes = unknown_asset_response
        .into_body()
        .collect()
        .await
        .expect("collect unknown asset body")
        .to_bytes();
    let unknown_asset_html =
        String::from_utf8(unknown_asset_bytes.to_vec()).expect("unknown asset html utf8");
    assert!(unknown_asset_html.contains("<div id=\"app\"></div>"));

    // Known embedded non-API assets are served directly with their own media type.
    let favicon_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/favicon.svg")
                .method("GET")
                .body(Body::empty())
                .expect("favicon request"),
        )
        .await
        .expect("favicon response");

    assert_eq!(favicon_response.status(), StatusCode::OK);
    assert_eq!(
        favicon_response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("image/svg+xml")
    );

    let unknown_api_response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/not-a-real-route")
                .method("GET")
                .body(Body::empty())
                .expect("unknown api request"),
        )
        .await
        .expect("unknown api response");

    assert_eq!(unknown_api_response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn health_endpoint_reports_ok_and_the_service_name() {
    // The gate fails closed on uninitialized auth state; the test-only
    // Disabled state preserves this test's health-shape assertions.
    backend::initialize_auth_disabled_for_tests();

    let app = backend::app_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/health")
                .method("GET")
                .body(Body::empty())
                .expect("health request"),
        )
        .await
        .expect("health response");

    assert_eq!(response.status(), StatusCode::OK);

    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    let json: Value = serde_json::from_slice(&bytes).expect("json body");

    assert_eq!(json["status"], "ok");
    assert_eq!(json["service"], "backend");
}
