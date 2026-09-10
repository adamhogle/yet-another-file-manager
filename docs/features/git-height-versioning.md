# Feature Spec: Git-Height Versioning

## Summary

Add a single repository-owned version source and a deterministic git-height version calculator so CI, Docker image tagging, and release automation can share one versioning model.

## Problem Statement

The repository currently has inconsistent manifest versions and no release-oriented versioning policy. That makes future image publishing and release automation ambiguous because there is no canonical version source.

## User Story

As a maintainer, I want one manually managed base version with automatically derived build versions, so that release metadata and Docker tags stay consistent without hand-editing multiple files per build.

## Scope

- In scope: add a single repo-level base version file.
- In scope: compute a full semver-compatible version using git height since the last base-version bump.
- In scope: expose the computed version through a script usable by CI.
- In scope: align existing manifest versions to the new base release line.
- In scope: document the versioning model for future image publishing.

## Non-Goals

- Not in scope: publishing container images.
- Not in scope: automatic release creation or tag creation.
- Not in scope: embedding runtime version display in the UI or backend API.

## UX / Flow

- Maintainer updates the repo version file when changing the major or minor release line.
- CI computes the effective build version from the base version file plus git height.
- Future image publishing can reuse the computed semver tag, SHA tag, and `latest` on `main`.

## Technical Notes

- Add a root `version.json` file as the canonical base version source.
- Add a Node script that reads `version.json`, finds the most recent commit that changed it, and uses the number of commits since then as the patch component.
- Keep Docker tag outputs free of build metadata so they are safe as image tags.
- Keep package manifests aligned to the base release line to avoid local confusion.

## Security Considerations

- Version calculation reads only local git metadata and repository-owned files.
- No secrets or remote network calls are required.
- CI outputs should remain deterministic for the checked-out commit.

## Acceptance Criteria

- [x] A single repo-level base version file exists and is documented.
- [x] A script can output the computed semver version and Docker-safe tags for the current commit.
- [x] CI uses the version script so future publish steps can consume consistent outputs.
- [x] Existing package manifests no longer disagree on the project release line.
- [x] Focused automated tests cover the version calculation logic.

## Test Plan

- Unit: verify version info generation and branch/tag outputs.
- Integration: run the version script in CI and local shell.
- End-to-end/manual: inspect CI logs for the resolved version values.

## Open Questions

- Whether release tags should later override computed versions on tagged commits can be added in a follow-up.
