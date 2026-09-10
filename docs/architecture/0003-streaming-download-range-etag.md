# ADR 0003: Streaming Download with Range and ETag Support

## Status

Accepted

## Context

The `/api/v1/download` endpoint read the entire shared file into memory before
responding (`fs::read`), so memory usage scaled with the largest file a client could
name. That is unbounded on the core use case of sharing files from a self-hosted root.
The response also carried no validators: no `Range` support for partial downloads and
no `ETag` for conditional requests, which download managers and resumable clients
expect.

The system handles sensitive file operations where correctness and explicit failure
behavior matter more than implementation speed, so the response semantics had to be
defined precisely rather than left to ad-hoc header handling.

## Decision

Stream downloads from an opened file handle and honor single-range conditional
semantics:

1. `GET /api/v1/download` opens the target with `tokio::fs::File` and streams the body
   through `tokio_util::io::ReaderStream` (64 KiB chunks) via `Body::from_stream`; the
   whole file is never buffered.
2. The `Range` header is parsed for a single `bytes` range only (`bytes=N-`,
   `bytes=N-M`, `bytes=-N`): a satisfiable range is answered with 206 and
   `Content-Range: bytes start-end/total`; an unsatisfiable range with 416 and
   `Content-Range: bytes */total`; an absent, non-bytes or malformed header is ignored
   and the full representation is served (200), per HTTP spec leniency.
   Multi-range requests are parked.
3. A weak ETag `W/"<hex>"` is computed from the file's mtime and length.
   `If-None-Match` is honored with weak comparison (→ 304); `If-Range` is honored only
   when its validator (entity tag or HTTP-date) matches. A non-matching `If-Range`
   widens the request back to the full representation.
4. 200 and 206 responses carry `Content-Length` (full or range length), the weak ETag
   and `Accept-Ranges: bytes`.

Response semantics are documented in the OpenAPI contract (206/304/416 on the download
path) and the download open step is kept as a named function so follow-up hardening
(verify-after-open identity checks, RFC 5987 `filename*`) slots into one place.

## Alternatives Considered

1. Keep whole-file buffering and add Range on top of the in-memory bytes
2. Implement multi-range / multipart responses
3. Serve ranges without validators (no ETag, no If-Range)

Whole-file buffering keeps the unbounded-memory problem and defeats the purpose of
Range support. Multi-range is unused by the UI and adds multipart assembly complexity.
Ranges without validators cannot be served safely after a representation changes and
do not let clients avoid re-downloads.

## Consequences

Benefits:

- Memory usage is bounded by the 64 KiB stream chunk size regardless of file size.
- Resumable and conditional downloads work with standard HTTP clients.
- Explicit 206/416 semantics make partial-download behavior predictable.

Trade-offs:

- Range and ETag handling add parsing and header-assembly code to the download path.
- Multi-range requests are ignored (full 200 response) until the parked work lands.
- The weak ETag cannot guarantee byte-identity across representation changes; it is
  paired with follow-up verify-after-open checks for that guarantee.

Follow-up work:

- Verify the opened handle after open (dev/ino + regular-file check) to close the
  canonicalize→read window.
- Add RFC 5987 `filename*` companions for non-ASCII download names.

## Security / Operations Impact

- Trust boundary remains at backend API; `Range` and conditional headers are untrusted
  input parsed defensively. Malformed input degrades to the full 200 response, never
  to an error leak.
- Streaming bounds peak memory on the shared-root host, so a request for a huge file
  no longer loads it whole.
- The 416 response reports only the representation size (`bytes */total`), not host
  paths.
- Regressions are guarded by a dedicated integration test binary asserting 206,
  Content-Range, 416, 304 and full-body semantics.
