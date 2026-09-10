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
    write_test_config(&config_path, &shared_root);

    backend::initialize_app_config(&config_path).expect("initialize app config");

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

    let first_entry = &json["entries"][0];
    assert_eq!(first_entry["name"], "demo.txt");
    assert_eq!(first_entry["kind"], "file");
    assert!(first_entry.get("sizeBytes").is_some());
    assert!(first_entry.get("modifiedAt").is_some());

    let invalid_response = app
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
}

#[tokio::test]
async fn embedded_frontend_serves_index_and_preserves_unknown_api_404() {
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
