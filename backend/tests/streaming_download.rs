use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use http_body_util::BodyExt;
use std::fs;
use std::path::Path;
use tempfile::TempDir;
use tower::util::ServiceExt;

const FILE_CONTENT: &[u8] = b"abcdefghij";
// The accented name exercises the RFC 5987 disposition. The content is written with
// its UTF-8 bytes because byte string literals hold ASCII only.
const ACCENTED_FILE_NAME: &str = "héllo.txt";
const ACCENTED_FILE_CONTENT: &[u8] = b"contenu accentu\xc3\xa9";

fn write_test_config(config_path: &Path, shared_root: &Path) {
    let config = format!("sharedRoot: {}\nshowHidden: false\n", shared_root.display());
    fs::write(config_path, config).expect("write test config");
}

async fn download_request(app: &Router, headers: &[(&str, &str)]) -> Response {
    let mut builder = Request::builder()
        .method("GET")
        .uri("/api/v1/download?p=range.txt");
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    app.clone()
        .oneshot(builder.body(Body::empty()).expect("build request"))
        .await
        .expect("download response")
}

async fn download_request_for(app: &Router, path: &str) -> Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(path)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("download response")
}

/// The download endpoint must stream and honor single Range requests
/// (206/Content-Range/416) plus ETag conditional headers. Runs in its own test binary
/// as one test so it gets its own process-global config: the api_contract binary
/// initializes a different temp root and the config OnceCell is first-call-wins.
#[tokio::test]
async fn download_serves_full_body_range_and_etag_semantics() {
    let temp = TempDir::new().expect("temp dir");
    let shared_root = temp.path().join("share");
    let config_path = temp.path().join("yafm.config.yaml");

    fs::create_dir_all(&shared_root).expect("create shared root");
    fs::write(shared_root.join("range.txt"), FILE_CONTENT).expect("write file");
    fs::write(shared_root.join(ACCENTED_FILE_NAME), ACCENTED_FILE_CONTENT)
        .expect("write accented file");
    fs::write(shared_root.join("quoted\"name.txt"), FILE_CONTENT).expect("write quoted file");
    fs::write(shared_root.join("my file.txt"), FILE_CONTENT).expect("write space file");
    write_test_config(&config_path, &shared_root);

    backend::initialize_app_config(&config_path).expect("initialize app config");
    // The gate fails closed on uninitialized auth state; the test-only
    // Disabled state preserves this binary's streaming coverage.
    backend::initialize_auth_disabled_for_tests();
    let app = backend::app_router();

    // Plain GET: 200 with the full body, a matching Content-Length and range support.
    let response = download_request(&app, &[]).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-length")
            .and_then(|value| value.to_str().ok()),
        Some("10")
    );
    assert_eq!(
        response
            .headers()
            .get("accept-ranges")
            .and_then(|value| value.to_str().ok()),
        Some("bytes")
    );
    // A 200 download is private: an authenticated user's file must not be cached
    // by an intermediary (the ETag already enables revalidation, so without this
    // heuristic caching is more likely).
    assert_eq!(
        response
            .headers()
            .get("cache-control")
            .and_then(|value| value.to_str().ok()),
        Some("private")
    );
    let etag = response
        .headers()
        .get("etag")
        .and_then(|value| value.to_str().ok())
        .expect("weak etag on a 200 response")
        .to_string();
    assert!(etag.starts_with("W/\""));

    let body = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    assert_eq!(&body[..], FILE_CONTENT);

    // Range: bytes=0-4 -> 206 with the first five bytes.
    let response = download_request(&app, &[("range", "bytes=0-4")]).await;
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        response
            .headers()
            .get("content-range")
            .and_then(|value| value.to_str().ok()),
        Some("bytes 0-4/10")
    );
    assert_eq!(
        response
            .headers()
            .get("content-length")
            .and_then(|value| value.to_str().ok()),
        Some("5")
    );
    let body = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    assert_eq!(&body[..], &FILE_CONTENT[..5]);

    // Range beyond the end of the file -> 416 with the total size in Content-Range.
    let response = download_request(&app, &[("range", "bytes=999999999-")]).await;
    assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(
        response
            .headers()
            .get("content-range")
            .and_then(|value| value.to_str().ok()),
        Some("bytes */10")
    );

    // If-None-Match with the served ETag -> 304.
    let response = download_request(&app, &[("if-none-match", &etag)]).await;
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);

    // Open-ended range reaches the final byte of the representation.
    let response = download_request(&app, &[("range", "bytes=5-")]).await;
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        response
            .headers()
            .get("content-range")
            .and_then(|value| value.to_str().ok()),
        Some("bytes 5-9/10")
    );
    let body = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    assert_eq!(&body[..], &FILE_CONTENT[5..]);

    // Suffix range serves the final N bytes.
    let response = download_request(&app, &[("range", "bytes=-3")]).await;
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        response
            .headers()
            .get("content-range")
            .and_then(|value| value.to_str().ok()),
        Some("bytes 7-9/10")
    );
    let body = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    assert_eq!(&body[..], &FILE_CONTENT[7..]);

    // Malformed and unsupported ranges are ignored: the full representation is served.
    for range in ["bytes=abc", "bytes=5-2", "items=0-4", "bytes=0-4,10-20"] {
        let response = download_request(&app, &[("range", range)]).await;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "range {range} should be ignored"
        );
        let body = response
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        assert_eq!(
            &body[..],
            FILE_CONTENT,
            "range {range} should serve everything"
        );
    }

    // If-Range with the served ETag serves the range; a non-matching validator
    // widens the request back to the full representation.
    let response = download_request(&app, &[("range", "bytes=0-4"), ("if-range", &etag)]).await;
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    let response = download_request(
        &app,
        &[("range", "bytes=0-4"), ("if-range", "W/\"nomatch-0\"")],
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    assert_eq!(&body[..], FILE_CONTENT);

    // A non-ASCII file name gets the RFC 5987 companion: an ASCII-safe percent-encoded
    // `filename` fallback plus `filename*=UTF-8''...`, and the header value stays
    // visible ASCII so any client can read it. The axum Query decodes the
    // percent-encoded request back to the accented name.
    let response = download_request_for(&app, "/api/v1/download?p=h%C3%A9llo.txt").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-disposition")
            .and_then(|value| value.to_str().ok()),
        Some("attachment; filename=\"h%C3%A9llo.txt\"; filename*=UTF-8''h%C3%A9llo.txt")
    );
    let body = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    assert_eq!(&body[..], ACCENTED_FILE_CONTENT);

    // The CR/LF/quote/backslash sanitization for the plain `filename` param stays
    // intact: a quoted name downloads under the plain form with the quote replaced.
    let response = download_request_for(&app, "/api/v1/download?p=quoted%22name.txt").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-disposition")
            .and_then(|value| value.to_str().ok()),
        Some("attachment; filename=\"quoted_name.txt\"")
    );
    let body = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    assert_eq!(&body[..], FILE_CONTENT);

    // A name with spaces passes through the sanitization untouched and downloads
    // under its exact name; the request URL encodes the space once.
    let response = download_request_for(&app, "/api/v1/download?p=my%20file.txt").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-disposition")
            .and_then(|value| value.to_str().ok()),
        Some("attachment; filename=\"my file.txt\"")
    );
    let body = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    assert_eq!(&body[..], FILE_CONTENT);
}
