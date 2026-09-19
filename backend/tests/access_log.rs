//! Verifies that the download and login flows emit nginx-style access lines
//! through the tracing stream. Runs in its own test binary so it gets its own
//! process-global config and its own tracing subscriber capture.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use tempfile::TempDir;
use tower::util::ServiceExt;

/// A tracing writer that appends formatted output to a shared buffer, so the
/// test can assert on the access lines the handlers emit.
#[derive(Clone)]
struct CaptureWriter(Arc<Mutex<Vec<u8>>>);

impl Write for CaptureWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn write_test_config(config_path: &Path, shared_root: &Path) {
    let config = format!(
        "sharedRoot: {}\nshowHidden: false\ntrustProxy: false\n",
        shared_root.display()
    );
    fs::write(config_path, config).expect("write test config");
}

#[tokio::test]
async fn downloads_and_logins_emit_nginx_style_access_lines() {
    // Replace the no-op global subscriber with a capturing one. A fresh binary
    // has no subscriber yet, so this takes effect for the whole test.
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let writer_buffer = buffer.clone();
    let _ = tracing_subscriber::fmt()
        .with_writer(move || CaptureWriter(writer_buffer.clone()))
        .try_init();

    let temp = TempDir::new().expect("temp dir");
    let shared_root = temp.path().join("share");
    let config_path = temp.path().join("yafm.config.yaml");
    fs::create_dir_all(&shared_root).expect("create share");
    fs::write(shared_root.join("demo.txt"), b"hello").expect("write file");
    write_test_config(&config_path, &shared_root);

    // The config OnceCell is first-call-wins in a process; using the test-only
    // Disabled auth state makes the download reachable without an IdP.
    backend::initialize_app_config(&config_path).expect("init config");
    backend::initialize_auth_disabled_for_tests();

    let app = backend::app_router();

    // A successful download emits a 200 line with the served byte count.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/download?p=demo.txt")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("download response");
    assert_eq!(response.status(), StatusCode::OK);
    let etag = response
        .headers()
        .get(header::ETAG)
        .and_then(|value| value.to_str().ok())
        .expect("a 200 download carries a weak ETag")
        .to_string();

    // A missing file is a 404 download outcome, also logged.
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

    // A range request is a 206 outcome with the range length as the byte count.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/download?p=demo.txt")
                .method("GET")
                .header(header::RANGE, "bytes=0-1")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("range response");
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);

    // An unsatisfiable range is a 416 download outcome with no body.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/download?p=demo.txt")
                .method("GET")
                .header(header::RANGE, "bytes=10-20")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("unsatisfiable range response");
    assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);

    // A matching If-None-Match is a 304 download outcome with no body.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/download?p=demo.txt")
                .method("GET")
                .header(header::IF_NONE_MATCH, &etag)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("not-modified response");
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);

    // A HEAD request is routed to the GET handler with the body removed, so
    // it logs zero bytes even though the handler promises a Content-Length.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/download?p=demo.txt")
                .method("HEAD")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("head response");
    assert_eq!(response.status(), StatusCode::OK);

    // A forged newline in the path passes validation (it is not a rejected
    // form) and misses the file lookup, so it is a 404 outcome. The rendered
    // line escapes it: one access line, no forged second line.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/download?p=a%0Ab")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("forged path response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // A failed login (auth disabled -> Unavailable 503) emits a LOGIN line.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/auth/callback?code=x&state=y")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("callback response");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

    // The handlers run in-process, so the capture buffer already holds the lines.
    let guard = buffer.lock().unwrap();
    let captured = String::from_utf8_lossy(&guard);

    assert!(
        captured.contains("\"DOWNLOAD demo.txt\" 200 5"),
        "expected a 200 download line, got:\n{captured}"
    );
    assert!(
        captured.contains("\"DOWNLOAD missing.txt\" 404 0"),
        "expected a 404 download line, got:\n{captured}"
    );
    assert!(
        captured.contains("\"LOGIN\" 503 0"),
        "expected a failed LOGIN line, got:\n{captured}"
    );
    assert!(
        captured.contains("\"DOWNLOAD demo.txt\" 206 2"),
        "expected a 206 line with the range length, got:\n{captured}"
    );
    assert!(
        captured.contains("\"DOWNLOAD demo.txt\" 416 0"),
        "expected a 416 line, got:\n{captured}"
    );
    assert!(
        captured.contains("\"DOWNLOAD demo.txt\" 304 0"),
        "expected a 304 line, got:\n{captured}"
    );
    assert!(
        captured.contains("\"DOWNLOAD demo.txt\" 200 0"),
        "expected the HEAD download to log zero bytes, got:\n{captured}"
    );
    assert!(
        captured.contains("\"DOWNLOAD a\\x0ab\" 404 0"),
        "expected the forged newline to render escaped, got:\n{captured}"
    );
    assert!(
        !captured.contains("DOWNLOAD a\nb"),
        "a forged newline split the access log"
    );
}
