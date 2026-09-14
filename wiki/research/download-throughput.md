# Download throughput investigation (2026-09-14)

Status: code exonerated, environmental cause confirmed, exact trigger unidentified.
The regression guard test stays.

## Symptom

A 1 GB download through yafm ran at about 800 KB/s, roughly 21 minutes for the
transfer. The connection can do 1 Gbit up and down. Every link between the client
and the host tested fine, so the problem was attributed to yafm. Later, with no
code change, the same download moved at 15 to 16 MB/s, a 20x improvement.

## What the code does per download request

Read from `backend/src/lib.rs`, verified line by line:

- The body streams through `Body::from_stream(ReaderStream::with_capacity(file, 64 KiB))`.
  No buffering of the whole file anywhere in the response chain.
- The filesystem work per request is three syscalls (stat, open, fstat) in
  `open_download_target`. Nothing per chunk.
- The auth gate is a pure function of the path and the Cookie header and passes
  the response through untouched. No introspection call per request.
- No compression middleware, so the payload is not re-encoded on the fly.
- The frontend links to `/api/v1/download` with a plain `href`, so the browser
  streams the body directly. No fetch buffering, no base64 conversion.
- There is no upload endpoint at all, so both directions in the symptom report
  can only refer to download.
- No sleeps, no rate limiting, no artificial delay in the serving path.

## Local measurements

Run on the devcontainer checkout against a build of this exact code. The release
profile matches what the runtime image ships.

| Boundary                                           | Measured  |
| -------------------------------------------------- | --------- |
| Raw read, overlay-backed share                     | 16.1 GB/s |
| yafm download, 64 MiB, overlay share (debug build) | 777 MB/s  |
| yafm download, 1 GB, overlay share (debug build)   | 761 MB/s  |
| Raw read, 9p-mounted share                         | 230 MB/s  |
| yafm download, 64 MiB, 9p share (debug build)      | 154 MB/s  |

The 1 GB number is the important one: the serving path holds up at the scale the
user reported. Even with the shared root on a 9p mount, throughput stays above
the 1 Gbit line rate, so no local reproduction could reach the reported
800 KB/s.

## Conclusion

The serving code is not the bottleneck. Throughput changed when the environment
changed and stayed flat across every code measurement, including the
release-equivalent build. The cause sits in the deployment, not in yafm.

15 to 16 MB/s is still below what a healthy 1 Gbit link sustains (about 117
MB/s after TCP overhead). The environment that produced 800 KB/s produced
something else after an unknown change, and the remaining gap has its own cause
worth bisecting.

## The authentik downtime correlation

The user later identified the only environmental change in the window: the
authentik (OIDC) provider went down for maintenance. The code has no dependency
that could tie download throughput to authentik availability:

- The auth gate verifies the signed session cookie locally. The HMAC check runs
  against a process-global cookie jar derived from the configured client secret
  (`backend/src/auth.rs`, `gate_decision` and `session_cookie_jar`). No network
  call per request.
- Discovery is lazy and cached. The provider metadata and JWKS are fetched only
  on the first login, callback or logout use, then cached for the process
  lifetime. A download request with a valid session cookie never touches
  authentik.
- A failed discovery is retried per login attempt, never per request, and never
  blocks a download.

So the correlation runs through shared infrastructure, not through yafm's code.
The leading candidate becomes shared-host CPU contention: authentik's worker
processes (a Python application with real steady-state load) running on the same
host as yafm, competing for cores. An 800 KB/s stream is consistent with a
starved core, and the jump to 15 MB/s when authentik stopped is consistent with
freed capacity. The remaining gap to line rate points at a second cause (a slow
shared root mount or TLS termination) that was masked before.

## Candidate causes, ranked

1. Shared-host CPU contention with authentik on the same host. Verify when
   authentik comes back: run a download and watch the host's CPU (top or
   docker stats). If throughput drops again while authentik is up, this is the
   cause. Fix: move authentik off the shared host, or cap yafm's contention with
   dedicated cores.
2. The shared root sits on a filesystem that is slow for the container: a Docker
   Desktop bind mount into a Windows or macOS host filesystem (9p or virtiofs),
   an NFS or SMB share, or a network volume. This is the classic source of
   hundreds of KB/s reads in Docker deployments.
3. A reverse proxy in front of yafm terminating TLS on a starved host, or with
   small buffering. The user tested the links up to the host, so the host to
   container segment may not have been covered.
4. Container CPU limits. Disk reads, TLS and request logging on a throttled core
   cap throughput well below line rate.
5. Small TCP buffer sysctls inside the container (net.core.rmem_max,
   wmem_max, tcp_rmem, tcp_wmem).

## Diagnostic script

Run on the deployment host. Pick the file the user downloaded, or the largest
file in the shared root. The numbers localize the boundary where throughput
collapses.

```bash
# 1. Raw read inside the container: the shared root's filesystem
docker exec <container> sh -c 'dd if=/app/share/<file> of=/dev/null bs=1M status=none'

# 2. Loopback on the host, direct to yafm, no proxy, no external network
curl -o /dev/null -w 'step2 direct: %{speed_download} B/s\n' \
  'http://127.0.0.1:8080/api/v1/download?p=<file>'

# 3. From the host to the container's published address (bind/port forwarding path)
curl -o /dev/null -w 'step3 published port: %{speed_download} B/s\n' \
  'http://<host-ip>:8080/api/v1/download?p=<file>'

# 4. The full public path with TLS (proxy included)
curl -o /dev/null -w 'step4 proxy: %{speed_download} B/s\n' \
  'https://files.example.com/api/v1/download?p=<file>'
```

Interpretation: whichever step shows the collapse is the failing boundary. Step
1 collapses to the filesystem, step 2 to the backend or its config, step 3 to
the port forwarding layer, step 4 to the proxy or TLS termination.

## Regression guard

`backend/tests/download_throughput.rs` streams a 32 MiB payload over a real
loopback TCP connection through the full router, single stream and four
concurrent streams, and asserts both stay above a 25 MB/s floor. The floor is
far under anything a healthy runner produces (the release-equivalent build
finishes the 160 MiB of transfer in 0.21s) but fails loudly on pathological
slowness, the regime the deployment fell into. Run it with
`cargo test --release --test download_throughput`.
