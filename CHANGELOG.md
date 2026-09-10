# Changelog

All notable changes to this project are documented in this file.

The format is based on Keep a Changelog, and this project uses calendar
versioning: `YYYY.MM.N`, where `N` is the commit count since the release
line was last set in `version.json`. See README, Versioning.

## [Unreleased]

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
