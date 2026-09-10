# ADR 0004: OIDC Authentication with All-or-Nothing Endpoint Gating

## Status

Accepted

## Context

The backend serves every endpoint to any caller: directory listings, downloads, and
the health probe are public. For a self-hosted file manager that is accidental data
disclosure: anyone who can reach the port can browse and download the shared root.
The hard requirement is that all endpoints are secured by auth checks and that the
requirement is verified, not just defaulted on. CONTRIBUTING.md item 4 already states
the direction: "Prefer server-side enforcement for file operations and access
control."

Deployment facts constrain the design. The runtime image is debian bookworm-slim with
only `ca-certificates` installed; TLS terminates at a reverse proxy outside the
container; the backend ships as a single stateless binary with no server-side session
infrastructure. The identity provider is authentik, whose behavior constrains the
flow:

- The default issuer mode is per-provider, `iss = https://authentik.company/application/o/<application_slug>/`, discoverable
  from the provider's `.well-known/openid-configuration`.
- Discovery, JWKS, and token-endpoint calls are direct server-to-server requests that
  reverse proxies, CDNs, and bot protection must permit (interactive challenges there
  cause repeated redirects).
- JWTs are signed RS256 when the provider has a Signing Key selected, and HS256 with
  the client secret when no Signing Key is configured.
- A client requesting no scopes is treated as requesting all configured scopes;
  `email_verified` defaults to false since authentik 2025.10.

## Decision

Enforce a server-side all-or-nothing auth gate on every endpoint, backed by an OIDC
authorization-code flow against authentik:

1. **The gate wraps every route + fallback, and the gate is the trust boundary.** No
   request reaches a handler, directory listing, download bytes, health JSON, SPA
   shell or assets, without passing the gate. This supports CONTRIBUTING.md item 4
   ("Prefer server-side enforcement for file operations and access control"): the
   check lives on the server, not in the browser.
2. **Auth is mandatory via config.** The `oidc` config block is required; startup
   aborts with a clear full-sentence error without it. A config-optional flag would
   let a misconfigured deployment silently run unauthenticated, violating the hard
   requirement. The only test backdoor is `YAFM_DISABLE_AUTH=1`, honored only when
   `cfg!(debug_assertions)`, development builds; the Dockerfile builds `--release`,
   so the release image can never accidentally run open. The hard requirement is
   verified explicitly by a gate-enumeration test asserting unauthenticated requests
   to every endpoint are rejected (401 for `/api/*`, 302 to login for non-API paths),
   not by config defaults.
3. **Stateless signed session cookies, signed with an independent key.** A signed
   session cookie (HttpOnly, SameSite=Lax, Secure=configurable, 12h TTL) is the
   only session store; there is no server-side session state. This fits the
   single-binary stateless deployment, an in-process memory store would lose sessions
   on every restart, and an external store would add a second stateful component.
   The signing key is independent of the OIDC client secret: it is the optional
   `oidc.sessionSigningKey` config value, expanded with SHA-256 to the master key
   length the `cookie` crate requires, or a random `cookie::Key::generate()` per
   process when omitted. The OIDC client secret must NOT be the signing material,
   it alone would be enough to mint valid 12-hour session cookies, and stateless
   signed cookies have no revocation path. With no `sessionSigningKey` configured,
   every restart generates a fresh key and invalidates outstanding sessions (the
   desired outcome after a compromise; re-login through authentik's SSO session is
   an instant redirect). Rotating the configured key has the same effect. A
   short-lived signed login cookie (~10 min) carries the OIDC state, the nonce, the
   PKCE verifier and the return-to path between the login and callback endpoints.
4. **rustls TLS for the OIDC HTTP client.** The runtime image installs only
   `ca-certificates`; an OpenSSL/curl TLS stack would compile but fail to load at
   runtime. The OIDC HTTP client (openidconnect over reqwest) uses rustls, so no new
   runtime packages are needed.
5. **Hybrid gate (path-based).** The gate returns 401 `PublicErrorResponse` JSON for
   `/api/*` paths and 302-redirects browsers to login for everything else (SPA shell,
   assets, downloads). Because downloads are plain `<a href>` navigations, a pure
   client-side 401 design would download the 401 JSON body as a file; redirecting
   `/api/*` too would make API fetches follow a cross-origin redirect chain to
   authentik, which same-origin-credential fetches cannot complete. The hybrid gives
   correct behavior for both navigation and fetch.
6. **Auth endpoints excluded from the OpenAPI contract.** Login, callback, and logout
   are browser-flow endpoints (302 redirects, no JSON bodies); the SPA uses browser
   navigations for them, not the generated client, and the client generator requires a
   200 response schema per path. Including them would force generator changes for zero
   client value; the exclusion is documented in the README and this ADR instead.

The auth endpoints themselves are `GET /api/v1/auth/login`,
`GET /api/v1/auth/callback`, and `GET /api/v1/auth/logout`: login generates state +
nonce + PKCE (S256), sets the short-lived signed login cookie, and redirects to
authentik's authorization endpoint; callback validates state/nonce, exchanges the code
server-to-server, validates the ID token (iss/aud/exp/nonce via the discovered JWKS),
and sets the session cookie; logout clears the session cookie and redirects to
authentik's end-session endpoint. Scopes are `[openid, profile, email]`. Provider
discovery is lazy (a `tokio::sync::OnceCell` on first use), startup makes no network
call, so container ordering cannot break startup. The gate fails closed (503) when
auth state is uninitialized.

## Alternatives Considered

1. Config-optional auth with a default-off flag
2. Pure client-side 401 handling for all paths
3. A server-side session store (tower-sessions memory store)
4. Including the auth endpoints in the OpenAPI contract
5. OpenSSL / native-tls for the OIDC HTTP client

Config-optional auth lets a misconfigured deployment silently run unauthenticated,
it violates the hard requirement; mandatory config plus the debug-gated env var keeps
the test path while making an open production run unreachable by construction. A pure
client-side 401 design downloads the 401 JSON as a file for download navigations.
A memory session store loses sessions on restart; an external store adds
infrastructure the single binary does not need. Including the auth endpoints in
`ApiDoc` would force generator changes for zero client value. An OpenSSL/curl stack
would compile but fail to load in the runtime image, which installs only
ca-certificates.

## Consequences

Benefits:

- Every endpoint is protected by construction, and the protection is asserted by an
  explicit gate-enumeration test rather than config defaults.
- No server-side session state; the single-binary stateless deployment is unchanged.
- The OIDC HTTP client needs no runtime libraries beyond ca-certificates.

Trade-offs:

- Every test binary that hits the router needs a one-line test-only auth
  initialization (the gate fails closed on uninitialized auth state).
- The backend cannot start without a valid `oidc` config block; local development
  without authentik needs the debug-gated `YAFM_DISABLE_AUTH=1` flag.
- Session expiry means re-login after 12h; refresh tokens are deliberately deferred
  until the TTL proves too short.
- The health endpoint is authenticated, so plain uptime probes from outside need a
  session or must move to the reverse-proxy layer.

Follow-up work:

- Per-user permissions (group claims / role checks), all-or-nothing only in v1.
- Refresh tokens / `offline_access` if session TTL proves too short.
- Back-channel logout handling (front-channel RP-initiated logout only today).

## Security / Operations Impact

- The trust boundary moves from "backend API is public" to "backend API is
  authenticated end-to-end": the gate is the single choke point for every route +
  fallback, and it fails closed (503) when auth state is uninitialized.
- The hard requirement is verified explicitly: a gate-enumeration test asserts
  unauthenticated requests to every endpoint are rejected (401 for `/api/*`, 302 to
  login for non-API paths), not by config defaults.
- Error responses never leak host paths or upstream details; discovery/token
  failures surface as 502/503 `PublicErrorResponse` ("The authentication provider is
  unavailable.").
- `returnTo` accepts only same-origin relative paths (starts with `/`, not `//`);
  anything else falls back to `/`, preventing open redirects.
- Observability: the gate runs inside tracing so 401/302 rejections are logged;
  rejected requests are otherwise indistinguishable in status from handler errors,
  which the gate-enumeration test and logs guard.
- Rollout: auth is mandatory via config, so an upgrade without an `oidc` block aborts
  startup with a clear error rather than serving unauthenticated traffic. The
  `YAFM_DISABLE_AUTH=1` backdoor is honored only in development builds
  (`cfg!(debug_assertions)`); the release image builds `--release`, so production can
  never run open. Reverse-proxy deployments must permit server-to-server
  discovery/JWKS/token calls and must set `cookieSecure: true` behind HTTPS.
- Logout CSRF is a documented residual risk: logout is a GET endpoint and
  SameSite=Lax sends cookies on top-level GET navigations, so a cross-site top-level
  navigation to `/api/v1/auth/logout` logs the victim out and ends their authentik
  SSO session. Pure nuisance, no data exposure; a POST change would break the plain
  `<a href>` logout in the hybrid gate (downloads are plain navigations), so the
  residual is documented deliberately instead of closed.
- No audit trail and no per-user revocation is a documented residual risk: the
  session payload is `v1:<exp>` only, no user identity, no per-request logging, and
  no way to invalidate one user's session. Embedding the ID token's subject in the
  signed payload plus per-request logging is follow-up work.
