use std::fs;
use std::net::SocketAddr;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::Instant;

// 32 MiB: large enough that per-request overhead and chunk boundaries cannot
// mask a throughput problem, small enough to keep the wall time CI-friendly.
const BENCH_SIZE: usize = 32 * 1024 * 1024;

// Floor the single-stream and concurrent measurements assert. Calibrated on a
// devcontainer where the same streaming path measured 777 MB/s with the shared
// root on an overlay-backed share and 154 MB/s with it on a 9p mount; a 1Gbit
// link needs about 125 MB/s. The floor sits well under both to stay green on
// slow CI runners while failing loudly on pathological slowness (sub-1 MB/s
// streaming would mean the deployment cannot fill even a 100 Mbit link).
const THROUGHPUT_FLOOR_MBPS: f64 = 25.0;

const CONCURRENCY: usize = 4;

/// Deterministic pseudo-random bytes via xorshift64 so the payload is not the
/// compressible all-zero pattern (a future compression middleware would make a
/// zeroed payload unrealistically fast).
fn bench_bytes(len: usize) -> Vec<u8> {
    let mut state = 0x2545F4914F6CDD1Du64;
    let mut data = Vec::with_capacity(len);
    for _ in 0..len {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        data.push(state as u8);
    }
    data
}

/// Position of the first `\r\n\r\n` (the response's header/body boundary). The
/// first occurrence is always the real one because headers precede the body.
fn find_header_terminator(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

/// Downloads `path` over a real loopback TCP connection and returns the body
/// length together with the achieved throughput in MB/s.
async fn download_via_tcp(address: SocketAddr, path: &str) -> Result<(usize, f64), String> {
    let started = Instant::now();
    let mut stream = TcpStream::connect(address)
        .await
        .map_err(|error| format!("connect to {address}: {error}"))?;

    let request = format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|error| format!("write request: {error}"))?;

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .map_err(|error| format!("read response: {error}"))?;

    let seconds = started.elapsed().as_secs_f64();
    let header_end = find_header_terminator(&response).ok_or("missing header terminator")?;
    if !response.starts_with(b"HTTP/1.1 200") {
        let status = String::from_utf8_lossy(&response[..header_end.min(response.len())]);
        return Err(format!("unexpected response: {status}"));
    }

    let body_len = response.len() - header_end - 4;
    let mbps = body_len as f64 / 1e6 / seconds;
    Ok((body_len, mbps))
}

/// The download endpoint must keep streaming at line rate for large bodies, both
/// for a single client and under concurrency, and the benchmark helper must
/// reject a non-200 response instead of counting an error body as transferred
/// bytes. Runs in its own test binary as one test so it gets its own
/// process-global config: the config OnceCell is first-call-wins and parallel
/// test threads inside one binary would race on it, so no second test function
/// initializes a different config here.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn download_streams_at_line_rate_single_and_concurrent() {
    let temp = TempDir::new().expect("temp dir");
    let shared_root = temp.path().join("share");
    let config_path = temp.path().join("yafm.config.yaml");

    fs::create_dir_all(&shared_root).expect("create shared root");
    // No bench.bin yet: the error-response phase below runs against the empty
    // share first, before the bench file exists on disk.
    fs::write(
        &config_path,
        format!("sharedRoot: {}\n", shared_root.display()),
    )
    .expect("write test config");

    backend::initialize_app_config(&config_path).expect("initialize app config");
    // The gate fails closed on uninitialized auth state; the test-only
    // Disabled state preserves this binary's throughput coverage.
    backend::initialize_auth_disabled_for_tests();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback listener");
    let address = listener
        .local_addr()
        .expect("bound listener has an address");
    tokio::spawn(async move {
        axum::serve(listener, backend::app_router())
            .await
            .expect("serve router");
    });

    // Error-response phase: a request for a missing file must surface an error
    // instead of being counted as transferred bytes by the benchmark helper.
    let error = download_via_tcp(address, "/api/v1/download?p=bench.bin")
        .await
        .expect_err("missing file must surface an error");
    assert!(error.contains("unexpected response"), "got: {error}");

    // Single client: the full 32 MiB body must arrive complete, at throughput
    // well above the floor. The bench file is written after the server started
    // because every download request stats the file from disk.
    fs::write(shared_root.join("bench.bin"), bench_bytes(BENCH_SIZE)).expect("write bench file");

    let (body_len, mbps) = download_via_tcp(address, "/api/v1/download?p=bench.bin")
        .await
        .expect("single-stream download");
    assert_eq!(body_len, BENCH_SIZE, "full body must be transferred");
    assert!(
        mbps >= THROUGHPUT_FLOOR_MBPS,
        "single-stream throughput {mbps:.1} MB/s fell below the {THROUGHPUT_FLOOR_MBPS} MB/s floor"
    );

    // Concurrent clients: parallel downloads must not starve each other (a
    // blocking-pool or runtime starvation would show up as an aggregate collapse).
    let started = Instant::now();
    let mut handles = Vec::new();
    for _ in 0..CONCURRENCY {
        handles.push(tokio::spawn(download_via_tcp(
            address,
            "/api/v1/download?p=bench.bin",
        )));
    }
    let mut total = 0usize;
    for handle in handles {
        let (body_len, _mbps) = handle
            .await
            .expect("concurrent task joined")
            .expect("concurrent download");
        assert_eq!(body_len, BENCH_SIZE, "full body must be transferred");
        total += body_len;
    }
    let concurrent_mbps = total as f64 / 1e6 / started.elapsed().as_secs_f64();
    assert!(
        concurrent_mbps >= THROUGHPUT_FLOOR_MBPS,
        "concurrent aggregate throughput {concurrent_mbps:.1} MB/s fell below the {THROUGHPUT_FLOOR_MBPS} MB/s floor"
    );
}
