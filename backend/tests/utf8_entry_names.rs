use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use tempfile::TempDir;
use tower::util::ServiceExt;

/// The listing must show percent-encoded names for entries whose names are not valid
/// UTF-8, and those entries must be re-openable through the API by that exact name.
/// Runs in its own test binary so it gets its own process-global config.
#[tokio::test]
async fn invalid_utf8_entry_names_are_listed_encoded_and_re_openable() {
    let temp = TempDir::new().expect("temp dir");
    let shared_root = temp.path().join("share");
    let config_path = temp.path().join("yafm.config.yaml");
    std::fs::create_dir_all(&shared_root).expect("create shared root");

    let config = format!("sharedRoot: {}\nshowHidden: false\n", shared_root.display());
    std::fs::write(&config_path, config).expect("write test config");

    let raw_name = OsStr::from_bytes(b"bad\xffname.txt");
    std::fs::write(shared_root.join(Path::new(raw_name)), "payload").expect("write file");

    backend::initialize_app_config(&config_path).expect("initialize app config");
    // The gate fails closed on uninitialized auth state; the test-only
    // Disabled state preserves this binary's UTF-8-name coverage.
    backend::initialize_auth_disabled_for_tests();
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
    let listed_name = json["entries"][0]["name"]
        .as_str()
        .expect("entry name")
        .to_string();
    assert_eq!(listed_name, "bad%FFname.txt");

    // Download by the listed name exactly as the client would send it: URLSearchParams
    // percent-encodes the value once ('%' -> "%25") and axum's Query decodes it back
    // once before validation.
    let mut query_value = String::new();
    for byte in listed_name.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'*' => {
                query_value.push(*byte as char)
            }
            b' ' => query_value.push('+'),
            _ => query_value.push_str(&format!("%{byte:02X}")),
        }
    }
    let uri = format!("/api/v1/download?p={query_value}");

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&uri)
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
    assert_eq!(&bytes[..], b"payload");

    // The encoded name refers to a file, so a directory request for it must 404.
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/directory?p=bad%25FFname.txt")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("directory response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
