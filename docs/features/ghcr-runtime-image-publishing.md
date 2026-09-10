# Feature Spec: GHCR Runtime Image Publishing

## Summary

Publish the runtime Docker image to GitHub Container Registry from CI using the repository's computed version outputs.

## Problem Statement

CI currently proves the Docker image can be built, but it does not produce a consumable runtime artifact for deployment or release validation.

## User Story

As a maintainer, I want successful `main` builds to publish a versioned runtime image to GHCR, so deployments can consume immutable images tied to repository history.

## Scope

- In scope: log in to GHCR from GitHub Actions using the workflow token.
- In scope: publish only the runtime image, not the devcontainer image.
- In scope: tag published images with full semver, release line, commit SHA, and `latest` on `main`.
- In scope: keep pull requests build-only with no registry writes.
- In scope: document the published image location in the README.

## Non-Goals

- Not in scope: signing images.
- Not in scope: multi-architecture publishing.
- Not in scope: release notes or GitHub Releases automation.

## UX / Flow

- Pull requests run the same image builds but do not publish to a registry.
- Pushes to `main` publish the runtime image to `ghcr.io/adamhogle/yet-another-file-manager`.
- Published tags include the computed semver tag, the release-line tag, a SHA tag, and `latest`.

## Technical Notes

- Use the version outputs from `scripts/compute-version.mjs`.
- Use `docker/login-action` with `GITHUB_TOKEN`.
- Grant `packages: write` only to the Docker publishing job.

## Security Considerations

- Registry publishing is restricted to trusted `push` events on `main`.
- Pull requests never receive package write access.
- Publishing uses the workflow token rather than a long-lived secret.

## Acceptance Criteria

- [x] CI publishes the runtime image on `main` push.
- [x] Published tags include semver, release line, SHA, and `latest`.
- [x] Pull request runs do not push images.
- [x] Documentation names the GHCR image location.

## Test Plan

- Unit: none beyond version-output coverage.
- Integration: verify CI workflow completes with a publish step on `main`.
- End-to-end/manual: inspect the GHCR package tags after a successful run.

## Open Questions

- Whether release tags should later be published in addition to git-height versions can be handled in a follow-up.
