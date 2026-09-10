# Yet Another File Manager

![Yet Another File Manager](frontend/public/yafm-logo.svg)

[![CI](https://github.com/adamhogle/yet-another-file-manager/actions/workflows/ci.yml/badge.svg)](https://github.com/adamhogle/yet-another-file-manager/actions/workflows/ci.yml)
[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0--only-blue)](https://www.gnu.org/licenses/agpl-3.0.txt)

A web-based file manager for self-hosted local file sharing, built with a Rust backend and Vue frontend.

## Runtime requirements

- Linux container runtime only (Docker deployment target)
- Windows host/container runtime is not supported

## Features

- Rust backend with the Vue frontend embedded in the binary.
- Directory listing and streaming downloads with Range and ETag support.
- Path traversal and symlink escape protection, verified by tests.
- OIDC authentication (authentik tested) with signed session cookies applied
  to every route, including static assets.
- OpenAPI contract generated from Rust types, with a generated TypeScript
  client and a CI drift check.
- Build, test, lint, and Docker image publishing in CI.

## Quickstart

The example config points `sharedRoot` at `../dev-data/share`, which the backend resolves relative to
its working directory (`backend/` when run through `npm run dev`), so create the shared directory
before the first run:

```sh
mkdir -p dev-data/share
cp config/yafm.config.example.yaml config/yafm.config.yaml
npm ci
npm run dev
```

The example config enables OIDC authentication. Developing without an identity provider
needs the test-only `YAFM_DISABLE_AUTH=1` flag, see [Authentication](#authentication).

## Runtime configuration

The backend expects a YAML or JSON runtime config file.

- The `backend` binary accepts an optional first CLI argument for the config file path.
- If the argument is omitted, it falls back to `config.yaml` then `config.json` in the current working directory.
- If neither file exists in the current working directory, startup fails.

Configuration fields:

- `sharedRoot` (required): absolute directory path to expose
- `showHidden` (optional): include hidden entries in listings. This is a LISTING
  filter, not an access control; hidden entries are still served on direct
  download (`/api/v1/download?p=.env` succeeds for an authenticated user who
  knows the name). Do not rely on it to protect secrets; keep sensitive files
  out of the shared root.
- `listenAddress` (optional, default `0.0.0.0`): backend bind address. Bind to loopback
  when running directly on a host behind a reverse proxy; `0.0.0.0` is the
  container-internal bind for Docker deployments.
- `listenPort` (optional, default `8080`): backend bind port
- `oidc.issuer` (required): the provider's issuer URL, from the provider's
  `.well-known/openid-configuration` (authentik's default issuer mode is per-provider:
  `.../application/o/<application slug>/`)
- `oidc.clientId` (required): OIDC client id registered at the provider
- `oidc.clientSecret` (required): OIDC client secret registered at the provider
- `oidc.sessionSigningKey` (optional): independent cookie-signing key for the session
  cookie; when omitted, a random key is generated per process and every restart
  invalidates outstanding sessions. Generate a stable key with `openssl rand -base64 32`;
  a configured key shorter than 32 bytes is refused at startup. The OIDC client secret
  must NOT be used here, the client secret alone would be enough to mint valid sessions.
- `oidc.redirectUri` (required): absolute https URL whose path is exactly
  `/api/v1/auth/callback`
- `oidc.cookieSecure` (optional, default `false`): must be `true` when `redirectUri` is https,
  startup refuses the mismatch. Set `false` explicitly only with a loopback http `redirectUri`
  for plain-HTTP local development.

## Authentication

Authentication is mandatory via config: the backend refuses to start without the `oidc`
block. Every endpoint (directory listings, downloads, health, SPA assets) is gated
server-side, unauthenticated `/api/*` requests get a 401 JSON error, and unauthenticated
browser navigations redirect through the login flow. The trust-boundary decision is
recorded in `docs/architecture/0004-oidc-authentication.md`.

### Setting up authentik

Create an OIDC provider application for the file manager (a confidential client with the
client id and secret the backend gets from the `oidc` config block):

- Register the redirect URI `https://<public-origin>/api/v1/auth/callback`, the path
  must be exactly `/api/v1/auth/callback`.
- The issuer comes from the provider's `.well-known/openid-configuration` document.
  Authentik's default issuer mode is per-provider:
  `https://authentik.company/application/o/<application slug>/`.

Then point the backend at it:

```yaml
oidc:
  issuer: https://authentik.company/application/o/yafm/
  clientId: yafm-spa
  clientSecret: replace-me
  redirectUri: https://files.example.com/api/v1/auth/callback
  # Set true when the site is served over HTTPS (TLS terminates at the reverse proxy).
  cookieSecure: true
```

See [Deployment](#deployment) for the full deployment expectations behind nginx +
authentik.

### Developing without authentik

Local development without an identity provider uses the test-only flag
`YAFM_DISABLE_AUTH=1`, honored only by development builds (`cfg!(debug_assertions)`;
`cargo run` is a debug build, release builds ignore the flag and always require the
`oidc` block):

```sh
YAFM_DISABLE_AUTH=1 npm run dev
```

With the flag set, the backend runs with the test-only disabled auth state and no `oidc`
block is needed.

## Scripts

```sh
npm run dev               # Build frontend and run Rust backend
npm run frontend:dev      # Run Vue dev server only
npm run check             # Cargo check + frontend build + typecheck
npm run typecheck         # vue-tsc typecheck over frontend/src and tests/
npm run contract:check    # Regenerate OpenAPI/client and fail on drift
npm run lint              # Prettier + ESLint
npm run test              # Cargo tests + integration tests
npm run build             # Build release Rust binary with embedded frontend
npm run version:print     # Resolve repo version from version.json + git height
```

## Versioning

The repository uses calendar versioning: `YYYY.MM.N`, where `YYYY.MM` is the
release line stored in `version.json` and `N` is the git commit count since the
last change to `version.json`.

- Bump `version.json` when you start a new month's release line (it resets the
  commit counter).
- `npm run version:print` shows the resolved build version for the current
  commit. `--plain` prints just the version, `--release` prints the release
  line version (`YYYY.MM.0`).
- The OpenAPI contract version in `api/openapi.yaml` is derived from the
  release line by `openapi:generate`.

Docker images use `YYYY.MM.N` tags, the `YYYY.MM` release line, a `sha-<sha>`
tag, and `latest` on main. Releases are tagged `vYYYY.MM.N` and published by
the Release workflow.

## Deployment

The backend is deployed behind a TLS-terminating reverse proxy (nginx in the reference
deployment) with authentik as the identity provider. Operators deploying behind nginx +
authentik should have no surprises; the expectations:

- **Serve the site at the domain root.** There is no sub-path/proxy-prefix support;
  `redirectUri` must be `https://<public-host>/api/v1/auth/callback` or startup aborts
  by design.
- **TLS at the proxy.** nginx terminates TLS and the backend serves plain HTTP; the
  backend trusts no `Host`/`X-Forwarded-*` headers, so header spoofing buys an attacker
  nothing; keep it that way.
- **`cookieSecure: true` behind HTTPS.** Startup refuses an https `redirectUri` paired
  with `cookieSecure: false` (a startup consistency check). Local development sets
  `false` explicitly with a loopback `redirectUri`.
- **Permit server-to-server calls** from the backend to authentik (discovery, JWKS,
  token endpoint). An interactive bot-protection challenge on that path breaks login,
  since the backend cannot follow redirects (redirects are disabled by design).
- **Health probes at the proxy layer.** `/api/v1/health` is authenticated (401
  unauthenticated by design; the SPA's 401 auto-recovery depends on it). Probe with
  nginx (TCP check or an upstream health module) rather than exempting the path.
- **Rate limiting at the proxy.** Put `limit_req` on `/api/v1/auth/` plus a sane global
  request limit: the auth endpoints are public and cheap to hammer (login mints a
  cookie and redirects on every hit), and a downed authentik plus no client timeout
  used to turn every login into a hanging call, the backend's OIDC connect and
  request timeouts now bound it. A sketch:

  ```nginx
  limit_req_zone $binary_remote_addr zone=auth:10m rate=10r/m;
  limit_req_zone $binary_remote_addr zone=global:10m rate=100r/m;

  server {
      listen 443 ssl;
      # TLS certificates and the security headers from below go here.

      location /api/v1/auth/ {
          limit_req zone=auth burst=20 nodelay;
          proxy_pass http://127.0.0.1:8080;
      }

      location / {
          limit_req zone=global burst=200 nodelay;
          proxy_pass http://127.0.0.1:8080;
      }
  }
  ```

- **Bind-address hardening.** `listenAddress: 0.0.0.0` (the default) is the
  container-internal bind for Docker deployments; running directly on a host behind a
  proxy, bind to loopback so direct backend access cannot bypass nginx TLS and rate
  limiting.
- **Read-only dedicated volume mount for the shared root.** The service never writes;
  mount the share read-only and as its own volume so hard links cannot cross
  filesystems into it. This closes the hard-link path (the Docker run already mounts
  `:ro`; the dedicated-volume part is the additive note). Mounting read-only also
  prevents a named pipe (FIFO) planted in the share from blocking a download request's
  tokio blocking-pool thread until a writer appears, a request-per-thread DoS under
  a writable share (the regular-file check runs after the open, so it cannot prevent
  the block).
- **Security headers at nginx.** Add what the app does not send: HSTS,
  `X-Content-Type-Options: nosniff` globally (the app sets it on downloads only),
  `X-Frame-Options: DENY` or `frame-ancestors 'self'`,
  `Referrer-Policy: strict-origin-when-cross-origin`, and a CSP for the SPA shell.
  The app already sends `Cache-Control`: `no-store` on the directory JSON and the
  SPA shell/assets, `private` on downloads. So the static bundle is deliberately not
  browser-cached; if shared/edge caching of assets matters, serve them from a separate
  `location` with its own cache directives.
- **Digest-pinned base images.** The Dockerfile's `node:22.23.2-bookworm-slim` and
  `debian:bookworm-slim` are mutable tags; pin by digest for reproducible builds.
- **Large downloads.** Disable `proxy_buffering` for `/api/v1/download` or size the
  buffers, and set `proxy_read_timeout` above the longest expected transfer (Range and
  If-Range pass through unchanged). `client_max_body_size` is irrelevant: there are no
  upload endpoints.
- **Residual risks (documented deliberately).** Logout CSRF: the GET logout endpoint is
  reachable by a cross-site top-level navigation (SameSite=Lax sends cookies on
  top-level GETs); pure nuisance, no data exposure; a POST change would break the
  plain `<a href>` logout in the hybrid gate. No user identity in the session payload
  and no per-user revocation (the payload is `v1:<exp>`; back-channel logout is
  documented as follow-up work in the ADR), an audit trail is follow-up work for a
  future version.

## Docker

Build the production runtime image:

```sh
docker build --target runtime -t yet-another-file-manager:local .
```

Run the container with a mounted config and shared directory. With no config argument, the
backend falls back to `config.yaml` in its working directory (`/app` in the container), so the
config mounts at `/app/config.yaml`. The example config's `sharedRoot: ../dev-data/share`
resolves to `/dev-data/share`, matching the second mount:

```sh
docker run --rm -p 8080:8080 \
	-v "$(pwd)/config/yafm.config.example.yaml:/app/config.yaml:ro" \
	-v "$(pwd)/dev-data/share:/dev-data/share:ro" \
	yet-another-file-manager:local
```

Build the devcontainer image (used by `.devcontainer/devcontainer.json`):

```sh
docker build -t yet-another-file-manager:devcontainer .
```

The CI workflow also builds this image on every push and pull request to `main`.

Pushes to `main` also publish the runtime image to GitHub Container Registry:

```sh
docker pull ghcr.io/adamhogle/yet-another-file-manager:latest
```

Published tags include the computed full version, release line, commit SHA tag, and `latest`.

## Governance

- Contribution rules: `CONTRIBUTING.md`
- Branch policy checklist: `docs/governance/BRANCH_PROTECTION_CHECKLIST.md`
- Pull request template: `.github/pull_request_template.md`
- Issue templates: `.github/ISSUE_TEMPLATE/`
- CI workflow: `.github/workflows/ci.yml`

## Copilot customization

Project-level AI guidance is defined in:

- `.github/copilot-instructions.md`
- `.github/instructions/`
- `.github/agents/`
- `.github/skills/`

See `docs/ai/COPILOT_STACK.md` for rationale and usage guidance.

## License

This project is licensed under the GNU Affero General Public License v3.0 only (AGPL-3.0-only).

Copyright 2026 Adam H. Ogle and contributors.

- Full license text: `LICENSE`
- Source repository: https://github.com/adamhogle/yet-another-file-manager
