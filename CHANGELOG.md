# Changelog

All notable changes to this project are documented in this file.

The format is based on Keep a Changelog, and this project uses calendar
versioning: `YYYY.MM.N`, where `N` is the commit count since the release
line was last set in `version.json`. See README, Versioning.

## [Unreleased]

### Added

- Added an nginx-style access log that emits a combined-log line for each login (success and failure) and each file download outcome in the existing tracing stream, under the `yafm::access` target. A new opt-in `trustProxy` config flag controls whether the logged client IP comes from `X-Forwarded-For` (behind a trusted reverse proxy) or the peer socket. User-controlled fields are escaped at render time, so a forged path or header cannot split a log line.

## [2026.09.20] - 2026-09-13

### Security

- Extended the gitleaks allowlist with value regexes for the test fixture signing key placeholder so the secret-scan job runs green on `main`.
- Updated dependencies across the stack: tokio 1.53.1, chrono 0.4.45, serde_json 1.0.151, tower-http 0.7.1, http-body-util 0.1.5, vue 3.5.42, @vitejs/plugin-vue, vitest 5.0.0, prettier 3.9.6, and the GitHub Actions majors (checkout 7, build-push-action 7, setup-buildx 4, setup-node 7, gitleaks-action 3), clearing the Node 20 deprecation warnings in CI.
- Added `@types/node` as a direct devDependency so regenerated lockfiles keep the Node type declarations.

### Changed

- Restructured the Dockerfile build stage into dependency layers so CI builds are incremental rather than clean rebuilds: manifests and lockfiles are baked into a cached layer first, and a source change recompiles only the backend crate.

### Fixed

- The docker-image CI job now loads the devcontainer image so the devcontainer toolchain assertion step can run it.
- Fixed a markdown list indentation flagged by prettier 3.9.6.

## [2026.09.0] - 2026-09-09

### Added

- Public release under AGPL-3.0-only.
- Calendar versioning (`2026.09.0`) replacing the previous 0.1.x scheme.
- CI secret scanning with gitleaks and dependabot update groups.
- Tag-driven release workflow: `vYYYY.MM.N` tags publish a GHCR image and a
  GitHub release.
- Security policy.

### Changed

- Repository opened to the public; history rewritten to milestone commits.
- README rewritten for readers landing from the public repo.
