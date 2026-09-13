# Feature Spec: GHCR Runtime Image Publishing

## Summary

Publish the runtime Docker image to GitHub Container Registry from CI using the repository's computed version outputs, and cut GitHub Releases from `v*` tags via the `release` workflow.

## Problem Statement

CI currently proves the Docker image can be built, but it does not produce a consumable runtime artifact for deployment or release validation.

## User Story

As a maintainer, I want successful `main` builds to publish a versioned runtime image to GHCR, so deployments can consume immutable images tied to repository history.

## Scope

- In scope: log in to GHCR from GitHub Actions using the workflow token.
- In scope: publish only the runtime image, not the devcontainer image.
- In scope: tag published images with the computed CalVer version (`YYYY.MM.N`), release line, commit SHA, and `latest` on `main`.
- In scope: keep pull requests build-only with no registry writes.
- In scope: document the published image location in the README.
- In scope: a `release` workflow triggered on `v*` tags that asserts the tag matches the computed version, publishes the image, and creates the GitHub Release.

## Non-Goals

- Not in scope: signing images.
- Not in scope: multi-architecture publishing.

## UX / Flow

- Pull requests run the same image builds but do not publish to a registry.
- Pushes to `main` publish the runtime image to `ghcr.io/adamhogle/yet-another-file-manager`.
- Published tags include the computed version tag, the release-line tag, a SHA tag, and `latest`. Intermediate per-commit build tags are not releases.
- Pushing a `v<version>` tag runs the `release` workflow: it verifies the tag matches `node scripts/compute-version.mjs --plain`, republishes the image pinned to that version plus `latest`, and creates a GitHub Release with generated notes.

## Technical Notes

- Use the version outputs from `scripts/compute-version.mjs`.
- Use `docker/login-action` with `GITHUB_TOKEN`.
- Grant `packages: write` only to the Docker publishing job.
- The release workflow uses a `release-${{ github.ref }}` concurrency group with `cancel-in-progress: false`, so concurrent release tags queue rather than cancel.
- Release versions use CalVer `YYYY.MM.N` where `N` is the commit count since `version.json` was last changed; see `docs/features/git-height-versioning.md`.

## Security Considerations

- Registry publishing is restricted to trusted `push` events on `main`.
- Pull requests never receive package write access.
- Publishing uses the workflow token rather than a long-lived secret.

## Acceptance Criteria

- [x] CI publishes the runtime image on `main` push.
- [x] Published tags include the version, release line, SHA, and `latest`.
- [x] Pull request runs do not push images.
- [x] Documentation names the GHCR image location.
- [x] A `v*` tag push creates a GitHub Release with a matching image.

## Test Plan

- Unit: none beyond version-output coverage.
- Integration: verify CI workflow completes with a publish step on `main`.
- End-to-end/manual: inspect the GHCR package tags after a successful run.

## Open Questions

- None.
