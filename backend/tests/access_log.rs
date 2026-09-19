//! Verifies that the download and login flows emit nginx-style access lines
//! through the tracing stream. Runs in its own test binary so it gets its own
//! process-global config and its own tracing subscriber capture.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
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
}
