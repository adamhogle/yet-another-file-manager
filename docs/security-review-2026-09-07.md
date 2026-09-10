# Security Review: yet-another-file-manager

- **Date:** 2026-09-07
- **Reviewer:** Independent verification pass (assistant), post 2026-09-06/07 OIDC and security-hardening work (commits `6a31492`..`1213b80`)
- **Deployment context reviewed:** reverse proxy with TLS termination (nginx reference), authentik OIDC, internet-facing
- **Result:** **No authentication bypass found.** One medium operational finding (JWKS rotation), two low findings, hardening notes. All 85 backend tests pass; the OpenAPI contract is unchanged.

## Method

- Full read of `backend/src/{auth,lib,main}.rs` and all seven integration test files (`auth_gate.rs`, `oidc_flow.rs`, `symlink_escape.rs`, `directory_listing.rs`, `streaming_download.rs`, `api_contract.rs`, `utf8_entry_names.rs`).
- Frontend source review (`frontend/src/**`, `tests/**`): XSS sinks, auth handling, generated client behavior.
- Live probing of the release binary against a scratch config: auth edge cases, traversal, symlinks, HTTP methods, open-redirect shapes, cookie flags, hidden-file semantics.
- Verification of openidconnect 4.0.1 crate internals from source (JWKS caching, ID-token validation).
- Dependency/advisory review (`Cargo.toml`, `deny.toml`, CI workflow).
- `cargo test`: 66 unit + 19 integration tests green; `YAFM_DISABLE_AUTH` release-build refusal verified live.

## Authentication Bypass Analysis (primary concern)

**No bypass found.** The auth model is a single choke-point middleware (`auth_gate`) applied to all routes including the fallback (`lib.rs:1211-1224`), and it fails closed. Live probes against the release binary:

| Probe                                                                                     | Result                                                                          |
| ----------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| `/api/v1/*` without cookie                                                                | 401 JSON                                                                        |
| `/`, assets, deep links without cookie                                                    | 302 to login (returnTo sanitized)                                               |
| Forged / tampered / garbage cookie values                                                 | 401 (tests also cover this exhaustively)                                        |
| Percent-encoded public path `/api/v1/auth/%6Cogin`                                        | **401, gated** (exact-match exemption, fail-safe)                               |
| Trailing slash `/api/v1/auth/login/`                                                      | 401, gated                                                                      |
| Case variant `/API/v1/auth/login`                                                         | 302 to login, never reaches handler                                             |
| Double slash `//api/v1/auth/login`                                                        | 302 to login (never matches login route)                                        |
| OPTIONS / POST / HEAD on data routes                                                      | 401 before method handling                                                      |
| Gate with uninitialized auth state                                                        | 503 fail-closed (tested in a spawned child process, OnceCells cannot be reset)  |
| `YAFM_DISABLE_AUTH=1` on release build                                                    | **refuses to start**, backdoor is `cfg!(debug_assertions)`-gated, verified live |
| Path traversal (`../`, `%2e%2e`, double-encoded `%252F`, absolute, backslash, NUL, mixed) | 400/404, all rejected                                                           |
| Symlink escaping the share (file + directory)                                             | 404, excluded from listings too                                                 |
| Open redirect via `returnTo` (`//evil`, `/\evil`, `https://evil`, `%2e%2e` tricks)        | All fall back to `/`; WHATWG dot-segment forms still resolve same-origin        |

Structural properties that make this solid:

- **Public-path exemption is exact-match** on the three auth endpoints only (`auth.rs:801-805`), the same constants used to register the routes, no drift possible.
- **A session cannot be minted without completing the full OIDC code exchange**: state (CSRF), PKCE S256, nonce, and ID-token signature/iss/aud/exp validation all verified (openidconnect 4.0.1 checks `exp`, `src/verification/mod.rs:828`; the mock-IdP E2E test exercises the real path).
- **Cookie signing key is independent of the client secret**, the secret alone cannot mint sessions. Secrets are redacted in `Debug` with tests pinning it.
- **No proxy-header trust**: `Host`/`X-Forwarded-*` are never read, so TLS termination at the proxy introduces no spoofing surface.
- **`cookieSecure: true` is enforced at startup** when `redirectUri` is https (`lib.rs:305-315`), the dangerous misconfiguration refuses to boot. Verified live.
- **All read-only**: no state-changing endpoints, so `SameSite=Lax` is sufficient CSRF protection; session cookies are `HttpOnly`, and session fixation is impossible (fresh cookie minted only in the callback).

The frontend is also clean: no `v-html`/`innerHTML` sinks, Vue escaping only, downloads forced to `application/octet-stream` + `attachment` + `nosniff` (no stored-XSS via file content), no tokens in localStorage.

## Findings

### 1. [Medium, operational] Authentik signing-key rotation breaks all new logins until restart

Most important new finding. Verified in the openidconnect 4.0.1 source:

- `discover_async` fetches the JWKS **once** during discovery (`src/discovery/mod.rs:333`);
- `id_token_verifier()` clones that key set into the verifier (`src/client.rs:642-652`), there is **no refetch on unknown `kid`**; the crate's own `NoMatchingKey` error docs say the client should refresh;
- the app caches the discovered provider for the **process lifetime** in an `AsyncOnceCell` (`auth.rs:663-680`), and the failed-discovery retry only applies before the first success.

Consequence: when authentik rotates its signing keys (certificate renewal, manual rotation), every new login fails ID-token validation with `503 provider unavailable` until the yafm process restarts. Existing sessions keep working, so this is an availability issue, not a bypass, but on an internet-facing deployment it will look like an outage.

**Recommendation:** either document "restart yafm after rotating authentik signing keys," or implement refresh: swap the `AsyncOnceCell` for a `RwLock`/TTL cache that re-runs discovery when ID-token validation fails with a key/signature error, and re-initialize the client. (Check with the authentik certificate renewal schedule, this will happen eventually.)

### 2. [Low] No strength requirement on `sessionSigningKey`

`lib.rs:316-321` accepts any non-blank string. The session payload is fully predictable (`v1:<unix-exp>`, `auth.rs:764-771`), so one observed cookie value gives an attacker a complete offline verification oracle: a weak passphrase key is brute-forceable, and success means the ability to mint arbitrary 12-hour sessions for anyone.

**Recommendation:** enforce a minimum (e.g. reject `sessionSigningKey` shorter than 32 bytes with a full-sentence startup error, consistent with the house style), since the README already prescribes `openssl rand -base64 32`. Defense-in-depth for a config-hygiene failure.

### 3. [Low] No `Cache-Control` on authenticated responses

Verified live: directory JSON, downloads, and the SPA shell all ship without `Cache-Control`. RFC-permitting intermediary caches (corporate proxies, some CDN configurations) may heuristically store authenticated responses. Downloads do carry `ETag`, which enables revalidation, making heuristic caching more likely.

**Recommendation:** add `Cache-Control: no-store` for `/api/v1/directory` (and `private` for downloads), or a blanket `no-store`/`private` policy at nginx. Cheap fix, removes a shared-cache leak of file listings/content.

### 4. [Low] `showHidden: false` is visibility-only, not access control

Verified live: `.env` hidden from the listing but direct download `?p=.env` returns 200. If an operator drops a `.git/` directory or `.env` into the share believing the flag protects them, it does not, anyone authenticated who guesses the name gets it.

**Recommendation:** document this explicitly in the README field description ("listing filter, not an access control"), or enforce it in `download`/`directory` when `showHidden: false` (product decision).

### 5. [Low, writable-share threat model] FIFO open hangs a blocking-pool thread

`open_download_target` (`lib.rs:589+`) opens the canonical path before verifying it is a regular file; opening a named pipe planted in the share blocks a `spawn_blocking` thread until a writer appears, a request-per-thread DoS. Same trust level as the documented hard-link and canonicalize-to-stat TOCTOU residuals (all require write access to the share; all mitigated by the documented read-only volume). The regular-file check does prevent serving `/dev/*` style streams.

**Recommendation:** nothing required if the share is mounted read-only (as documented). If a writable share is ever served, consider `O_NONBLOCK`/`openat2(RESOLVE_IN_ROOT)`, the latter also closes the acknowledged TOCTOU window.

## Documented Residuals (acknowledged, no action needed)

These are already deliberately recorded in ADR-0004 and the README; the reasoning holds:

- **Logout CSRF** (GET logout + `SameSite=Lax` top-level navigations), nuisance only, no data exposure.
- **No per-user identity or revocation**, session is `v1:<exp>` only; signing-key rotation is the only kill switch. 12h TTL bounds exposure; back-channel logout is listed follow-up.
- **No app-level rate limiting / public health endpoint**, delegated to nginx `limit_req` and proxy-level probes, with working config sketches in the README.
- **Security headers (HSTS/CSP/X-Frame-Options) delegated to nginx**, reasonable; `SameSite=Lax` already blocks cookie-bearing cross-site iframes.
- **Base images tag- not digest-pinned**, noted in the README as an operator action.
- **rsa 0.9.10 Marvin-attack advisory ignore**, justification in `deny.toml` is correct (public-key verification only; the attack targets private-key operations).

## Deployment Notes (nginx TLS + authentik)

The guards align well with this deployment model, startup enforces https `redirectUri` with `cookieSecure: true` and the exact callback path, which makes the dangerous proxy misconfigurations refuse to boot. Three things to verify on the operator side:

1. **Do not publish the backend port.** The README Docker example uses `-p 8080:8080`; on an internet-facing host prefer `-p 127.0.0.1:8080:8080` (or a Docker network). Auth still fully applies to a directly exposed port, and `Secure` cookies mean auth cannot even complete over plain HTTP, but a direct exposure would bypass TLS/rate limits.
2. **Permit backend-to-authentik server-to-server calls** (discovery/JWKS/token), no interactive bot protection on that path, and no redirects (the OIDC client has redirects disabled by design, good SSRF posture).
3. **Set the nginx `limit_req` zones from the README**, the login endpoint mints a signed cookie on every unauthenticated hit, so it is cheap to hammer; the OIDC client's 10s/30s timeouts (`auth.rs:121-131`) bound the worst case to a 503.

Also: access logs (tower-http trace layer) include the request URI with the `p` query, file paths land in backend logs. Fine for most operators; do not ship those logs somewhere more public than the files themselves.

## What Is Done Notably Well

- Gate-enumeration as an explicit hard requirement (`auth_gate.rs`), including a spawned-child-process test for the fail-closed uninitialized state, unusual rigor.
- The hybrid gate (401 JSON for `/api/*`, 302 for navigations) means downloads-as-navigations re-authenticate transparently.
- Verify-after-open (dev/ino identity check) narrows the TOCTOU window beyond typical implementations.
- Hand-written `Debug` redaction for both secret-bearing config structs, with tests in both binaries.
- Multi-layer path validation where each layer re-validates decoded forms (`percent_decode_relative_path` re-checks for encoded `/`, `\`, NUL, `..`).
- Non-root runtime user, read-only volume guidance, `cargo deny` + `npm audit` in CI.

## Bottom Line

The authentication boundary is solid and internet-ready as designed; no bypass exists. Before going live, address the JWKS-rotation restart behavior (finding 1) and ideally the signing-key strength check (finding 2); the rest are hardening polish.
