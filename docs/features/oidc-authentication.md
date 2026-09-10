# Feature Spec: OIDC Authentication

## Status

Done (2026-09-07)

## Summary

OIDC authentication with authentik as the identity provider: the site redirects the
browser to authentik for login, the backend exchanges the authorization code
server-to-server, and the resulting authenticated session is enforced on every
endpoint. All-or-nothing access — no per-user permissions in v1.

## Problem Statement

The backend currently serves every endpoint to any caller: directory listings,
downloads, and the health probe are all public. For a self-hosted file manager that is
accidental data disclosure — anyone who can reach the port can browse and download the
shared root. The requirement is actual OIDC integration: the site redirects for login,
that authentication is used on the endpoint, and all endpoints are secured by auth
checks as a hard requirement that is verified to prevent accidental data disclosure.

`docs/features/secure-directory-listing.md` lists "authentication or multi-user access
control" as not-in-scope; this feature supersedes that boundary for authentication
(the historical spec's text is left untouched as a record).

## User Story

As a user of the file manager, I want unauthenticated visits to be redirected to
authentik to log in, so that after returning with a session every endpoint (directory
listings, downloads, health, SPA assets) only serves data to authenticated requests.

## Scope

- In scope: OIDC authorization-code flow (confidential client) against authentik —
  login, callback, and logout endpoints with state + nonce + PKCE (S256) and ID token
  validation via the discovered JWKS.
- In scope: a server-side auth gate wrapping every route + fallback, path-based: 401
  JSON for `/api/*`, 302 redirect for browser navigations.
- In scope: an explicit hard-requirement verification test enumerating every endpoint
  and asserting unauthenticated requests are rejected, plus authenticated-flow tests
  and a mock-IdP end-to-end test.
- In scope: a nested `oidc` config block (`issuer`, `clientId`, `clientSecret`,
  `redirectUri`, `cookieSecure`), example config, README config section, and ADR 0004.
- In scope: a stateless signed session cookie (no server-side store; fits the
  single-binary stateless deployment).
- In scope: frontend 401 auto-recovery (probe-on-error in the wrapper client).
- In scope: test backdoors — a test-only auth-disabled state (Rust tests) and a
  debug-assertions-gated `YAFM_DISABLE_AUTH` env var (vitest integration spec).

## Non-Goals

- Not in scope: per-user permissions, group claims, or role checks — all-or-nothing
  only (explicit user boundary).
- Not in scope: refresh tokens / `offline_access` scope (authentik SSO makes re-login
  an instant redirect; revisit if the session TTL proves too short).
- Not in scope: back-channel logout handling (front-channel RP-initiated logout only).
- Not in scope: hot-reloading auth config (config is fixed for the process lifetime,
  as today).
- Not in scope: sub-path/proxy-prefix deployment support (the backend serves `/`
  directly).
- Not in scope: changes to the OpenAPI contract chain — the auth endpoints are
  browser-flow endpoints (302 redirects, no JSON bodies) and are deliberately excluded
  from `ApiDoc`; `openapi.yaml` and the generated client stay untouched.

## UX / Flow

Happy path:

1. The user opens the site (`/`). The auth gate finds no valid session cookie and 302s
   to `GET /api/v1/auth/login`.
2. The login endpoint validates `returnTo`, generates state + nonce + PKCE verifier,
   sets a short-lived signed login cookie, and 302s the browser to authentik's
   authorization endpoint.
3. The user authenticates at authentik; authentik redirects back to the configured
   `redirectUri` (`GET /api/v1/auth/callback?code=…&state=…`).
4. The callback validates state/nonce, exchanges the code at the token endpoint
   (server-to-server, rustls TLS), validates the ID token (iss/aud/exp/nonce via the
   discovered JWKS), sets the signed `yafm_session` cookie, clears the login cookie,
   and 302s to the original `returnTo` (or `/`).
5. The browser navigates to `/` with the session cookie; the gate passes; every
   endpoint works. Downloads (`<a href>` navigations) and same-origin API fetches ride
   the cookie automatically.
6. Logout (`GET /api/v1/auth/logout`) clears the session cookie and redirects to
   authentik's end-session endpoint.

The gate is hybrid and path-based: unauthenticated `/api/*` requests get a 401 JSON
error; unauthenticated browser navigations (SPA shell, assets, downloads) get a 302
redirect to login. Downloads are plain `<a href>` navigations, so a pure client-side
401 design would download the 401 JSON body as a file; redirecting `/api/*` too would
break same-origin fetch credential flows. When a session expires while a tab is open,
the SPA shell navigation and downloads redirect through login automatically, and
in-flight API fetches get a 401 that the wrapper resolves by probing health and
redirecting the browser to login.

Failure modes:

- Uninitialized auth state: the gate fails closed with 503 — it never serves data.
- Garbage, forged, or expired session cookie: rejected as unauthenticated (401 for
  `/api/*`, 302 for non-API paths) without a panic.
- Missing or incomplete `oidc` config block: startup aborts with a clear
  full-sentence error.
- Callback receives an OAuth `error` parameter (denied access, etc.): a safe 401 with
  no host-path or upstream-detail leaks.
- authentik unavailable: discovery/token calls fail with 502/503
  `PublicErrorResponse` ("The authentication provider is unavailable."), never a raw
  upstream error.
- Reverse proxy deployment: the backend sees plain HTTP and knows no public origin;
  `redirectUri`, `issuer`, and the cookie `Secure` flag are config fields.

## Technical Notes

- authentik specifics: the default issuer mode is per-provider
  (`iss = https://authentik.company/application/o/<application_slug>/`), discoverable
  from the provider's `.well-known/openid-configuration`; discovery, JWKS, and token
  calls are direct server-to-server requests that reverse proxies must permit (no
  interactive challenges); JWTs are signed RS256 when the provider has a Signing Key
  selected and HS256-with-secret when none is configured; the client requests
  `[openid, profile, email]` explicitly (a no-scope request is treated as requesting
  all configured scopes); `cookieSecure` defaults to false and must be true behind
  HTTPS.
- Stateless signed session cookie via the `cookie` crate, HttpOnly, SameSite=Lax,
  Secure=configurable, 12h TTL; a short-lived signed login cookie (~10 min) carries
  state + nonce + PKCE verifier + `returnTo`. The signing key is independent of the
  OIDC client secret: the optional `oidc.sessionSigningKey` config value (SHA-256-
  expanded to the `cookie` crate's master key length) or, when omitted, a random
  `cookie::Key::generate()` per process — every restart without a configured key
  invalidates outstanding sessions (SSO re-login is an instant redirect). The client
  secret alone can no longer mint valid sessions; rotating the key invalidates every
  outstanding session.
- `openidconnect` 4.x with rustls TLS; lazy discovery via a `tokio::sync::OnceCell`
  (startup makes no network call, so container ordering cannot break startup).
- Auth is mandatory via config; the only test backdoor is the debug-gated
  `YAFM_DISABLE_AUTH=1` env var, honored only in development builds (`cfg!(debug_assertions)`;
  the Dockerfile builds `--release`, so production can never run open).
- The health endpoint is authenticated — nothing in the Dockerfile, CI, or scripts
  depends on health being public.
- The hard requirement (all endpoints secured) is verified explicitly by a
  gate-enumeration test asserting unauthenticated requests to every endpoint are
  rejected (401 for `/api/*`, 302 to login for non-API paths) — not by config
  defaults.
- The gate fails closed (503) when auth state is uninitialized.
- `returnTo` open-redirect protection: it must be a same-origin relative path (starts
  with `/`, not `//`); anything else falls back to `/`.
- The trust-boundary decision is recorded in
  `docs/architecture/0004-oidc-authentication.md`.

## Security Considerations

- The auth gate is the trust boundary: every endpoint (directory listings, downloads,
  health, SPA shell and assets) is checked server-side; no directory bytes, health
  JSON, or SPA asset reaches an unauthenticated request.
- Session cookies are HttpOnly and signed; the payload carries only a version and an
  expiry, so forged or expired cookies are rejected as unauthenticated.
- state + nonce (CSRF) and PKCE (S256) protect the authorization-code flow; the code
  verifier travels only in the signed login cookie, never in URLs or logs.
- ID token validation is standard (iss, aud, exp, nonce) via the discovered JWKS;
  `email_verified` is not validated (authentik defaults it to false since 2025.10).
- Error responses never leak host paths or upstream details; an OAuth `error`
  parameter at the callback produces a safe 401.
- No per-user permission logic anywhere — all-or-nothing only.
- `returnTo` is validated as a same-origin relative path (starts with `/`, not `//`);
  anything else falls back to `/`, preventing open redirects.

## Acceptance Criteria

- [ ] Unauthenticated requests to every endpoint are rejected — 401 for `/api/*`, 302
      to login for non-API paths — asserted by an explicit test enumerating all paths.
- [ ] Login 302s the browser to the authentik authorization endpoint with a signed
      state + nonce + PKCE cookie set.
- [ ] Callback validates state + nonce, exchanges the code, and validates the ID
      token (iss, aud, exp, nonce) before issuing a session.
- [ ] A valid session cookie grants access to every endpoint (health, directory,
      download, SPA shell + assets).
- [ ] Invalid, forged, or expired session cookies are rejected as unauthenticated
      without a panic.
- [ ] Missing or incomplete `oidc` config block aborts startup with a clear error.
- [ ] Logout clears the session cookie and redirects to authentik's end-session
      endpoint.
- [ ] Login/callback/logout work behind a reverse proxy with configurable
      `redirectUri` and cookie `Secure` flag.
- [ ] Gate fails closed (503) when auth state is uninitialized.
- [ ] `returnTo` open-redirect protection: non-same-origin values fall back to `/`.
- [ ] Callback with an OAuth `error` parameter returns a safe 401 without leaking
      details.
- [ ] The runtime TLS stack is rustls-based — `cargo tree -e normal -i openssl-sys`
      (and curl-sys) report nothing.

## Test Plan

- Unit: cookie mint/verify round trips (valid, tampered signature, expired expiry,
  garbage input); gate behavior per state (disabled pass-through, uninitialized 503,
  unauthenticated 401 for `/api/*` and 302 for non-API paths, valid minted cookie
  pass-through); `returnTo` validation; exempt-path handling.
- Integration: `backend/tests/auth_gate.rs` — the explicit enumeration of every
  endpoint asserting unauthenticated rejection (401 for `/api/*`, 302 to login for
  non-API paths), forged/garbage session cookies, and a valid minted session granting
  access to every endpoint.
- End-to-end/manual: a mock-IdP full-flow test (login → authorize → callback →
  session → protected request) in `backend/tests/oidc_flow.rs`; manual verification
  against a real authentik instance.

## Open Questions

- Consent page behavior and exact claim shapes cannot be verified without a live
  authentik instance; the backend validates standard claims and is all-or-nothing, so
  group shapes do not affect the gate.
- Whether the 12h session TTL proves too short in practice (revisit refresh tokens
  then).

## Validation Evidence

- `cargo fmt --check` and `cargo clippy -- -D warnings` (backend/) — clean.
- `cargo test` (backend/) — every test binary passes: 59 lib tests, 3 main.rs, 3
  api_contract, 6 auth_gate (the explicit gate-enumeration and fail-closed
  enumeration), 3 directory_listing, 1 streaming_download, 1 symlink_escape, 1
  utf8_entry_names, and 1 oidc_flow (the mock-IdP full loop). Total 78 tests, 0
  failed.
- `cargo deny --manifest-path backend/Cargo.toml check` — advisories, bans,
  licenses, sources all ok.
- `cargo tree -e normal -i openssl-sys` and `-i curl-sys` — nothing (the runtime
  TLS stack is rustls-only).
- `vue-tsc --noEmit`, eslint, and prettier `--check .` — clean.
- Vitest: 19 tests across 4 files pass, including
  `tests/generated-client.integration.spec.ts`: the spawned backend carries the
  `YAFM_DISABLE_AUTH: '1'` spawn flag and serves health 200 under the test-only
  Disabled state (auth is mandatory via config, so the flag is the only way the
  spec's no-oidc-block config starts).
- `vite build` + `copy-public.mjs` — the production frontend bundle builds and
  copies the public assets.
- `contract:generate` + `contract:check` — idempotent, no drift (the OpenAPI
  contract and the generated client are untouched by this feature).
