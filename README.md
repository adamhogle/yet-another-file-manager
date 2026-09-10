# Yet Another File Manager

A web-based file manager for self-hosted local file sharing, built with a Rust backend and Vue frontend.

## Runtime Requirements

- Linux container runtime only (Docker deployment target)
- Windows host/container runtime is not supported

## Status

Current baseline includes:

- Rust backend API with embedded frontend static assets
- Vue frontend application
- OpenAPI generation from Rust endpoints and generated frontend API client
- Contract drift checks in CI
- Cargo tests plus frontend integration tests via Vitest
- Initial governance and CI workflows
- Copilot repo customizations for consistent AI-assisted development

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
needs the test-only `YAFM_DISABLE_AUTH=1` flag — see [Authentication](#authentication).

## Runtime Configuration

The backend expects a YAML or JSON runtime config file.

- The `backend` binary accepts an optional first CLI argument for the config file path.
- If the argument is omitted, it falls back to `config.yaml` then `config.json` in the current working directory.
- If neither file exists in the current working directory, startup fails.

Configuration fields:

- `sharedRoot` (required): absolute directory path to expose
- `showHidden` (optional): include hidden entries in listings
- `listenAddress` (optional, default `0.0.0.0`): backend bind address
- `listenPort` (optional, default `8080`): backend bind port
- `oidc.issuer` (required): the provider's issuer URL, from the provider's
  `.well-known/openid-configuration` (authentik's default issuer mode is per-provider:
  `.../application/o/<application slug>/`)
- `oidc.clientId` (required): OIDC client id registered at the provider
- `oidc.clientSecret` (required): OIDC client secret registered at the provider
- `oidc.sessionSigningKey` (optional): independent cookie-signing key for the session
  cookie; when omitted, a random key is generated per process and every restart
  invalidates outstanding sessions. Generate a stable key with `openssl rand -base64 32`.
  The OIDC client secret must NOT be used here — the client secret alone would be
  enough to mint valid sessions.
- `oidc.redirectUri` (required): absolute https URL whose path is exactly
  `/api/v1/auth/callback`
- `oidc.cookieSecure` (optional, default `false`): must be `true` when `redirectUri` is https —
  startup refuses the mismatch. Set `false` explicitly only with a loopback http `redirectUri`
  for plain-HTTP local development.

## Authentication

Authentication is mandatory via config: the backend refuses to start without the `oidc`
block. Every endpoint (directory listings, downloads, health, SPA assets) is gated
server-side — unauthenticated `/api/*` requests get a 401 JSON error, and unauthenticated
browser navigations redirect through the login flow. The trust-boundary decision is
recorded in `docs/architecture/0004-oidc-authentication.md`.

### Setting up authentik

Create an OIDC provider application for the file manager (a confidential client with the
client id and secret the backend gets from the `oidc` config block):

- Register the redirect URI `https://<public-origin>/api/v1/auth/callback` — the path
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

Deployment notes:

- Discovery, JWKS, and token calls are direct server-to-server requests to the provider
  — reverse proxies must permit them (no interactive challenges).
- `cookieSecure: true` behind HTTPS (TLS at the reverse proxy) — startup refuses an https
  `redirectUri` paired with `cookieSecure: false`. Set `false` explicitly only with a loopback
  `redirectUri` for plain-HTTP local development.

### Developing without authentik

Local development without an identity provider uses the test-only flag
`YAFM_DISABLE_AUTH=1`, honored only by development builds (`cfg!(debug_assertions)`;
`cargo run` is a debug build — release builds ignore the flag and always require the
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

The repository uses a single base version file at `version.json`.

- `major` and `minor` are changed manually.
- `patch` is derived from the git commit height since the last change to `version.json`.
- `npm run version:print` shows the resolved build version for the current commit.
- `node scripts/compute-version.mjs --plain` prints only the computed version, and `--release` prints the release-line version (`major.minor.0`).
- The OpenAPI contract version in `api/openapi.yaml` is derived from `version.json`'s release line by the `openapi:generate` script.

This version source is intended to drive future Docker image tags and release automation.

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

## Copilot Customization

Project-level AI guidance is defined in:

- `.github/copilot-instructions.md`
- `.github/instructions/`
- `.github/agents/`
- `.github/skills/`

See `docs/ai/COPILOT_STACK.md` for rationale and usage guidance.

## License

This project is licensed under the GNU Affero General Public License v3.0 only (AGPL-3.0-only).

- Full license text: `LICENSE`
- Source repository: https://github.com/adamhogle/yet-another-file-manager
