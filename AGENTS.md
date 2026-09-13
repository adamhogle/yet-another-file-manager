# AGENTS.md

Guidance for AI coding assistants working in this repository. A fuller Copilot-specific stack lives in `.github/copilot-instructions.md` and `.github/instructions/`; the conventions below apply to any assistant.

## Product Overview

YAFM is a self-hosted web file manager with a Rust backend API, a Vue 3 frontend, and OIDC authentication. Licensed AGPL-3.0-only. The production target is Linux Docker containers only; Windows runtime is out of scope.

## Layout

| Path                              | What it is                                                                                                                                  |
| --------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| `backend/`                        | Rust server, single crate. `src/lib.rs` (app and config), `src/main.rs` (bin), `src/auth.rs` (OIDC), `src/bin/openapi.rs` (spec generator). |
| `frontend/`                       | Vue 3, TypeScript, zod. Own `package.json`.                                                                                                 |
| `api/openapi.yaml`                | Contract source of truth. The typed client in `frontend/src/lib/api/` is generated from it.                                                    |
| `tests/`                          | Vitest specs, including the generated-client integration spec.                                                                              |
| `scripts/compute-version.mjs`     | Derives the version from git height.                                                                                                        |
| `config/yafm.config.example.yaml` | Server configuration example.                                                                                                               |
| `docs/architecture/`              | Architecture records (ADRs).                                                                                                                |
| `docs/features/`                  | Feature specs. Templates in `docs/templates/`.                                                                                              |
| `wiki/`                           | Plans and research notes.                                                                                                                   |

## Commands

- Root: `npm run check` (typecheck + tests), `npm run lint` (prettier + eslint), `npm run test`, `npm run typecheck` (vue-tsc), `npm run test:integration` (needs a live server).
- Backend: from `backend/`, `cargo build --release`, `cargo test --release --lib`.
- Frontend assets: `npm run frontend:build`.
- Contract work: `npm run openapi:generate`, `npm run client:generate`, gate is `npm run contract:check`.
- Root-only vitest needs the frontend deps installed first (`cd frontend && npm ci`); zod is a frontend-only dependency, so a fresh worktree fails until they are installed.
- On 9p checkouts (Windows Docker Desktop), `node_modules/.bin` is absent; invoke tools directly (`node node_modules/prettier/bin/prettier.cjs`, `node node_modules/vitest/vitest.mjs`). See `CONTRIBUTING.md`.

## Versioning

CalVer: `YYYY.MM.N`, where `N` is the commit count since `version.json` was last changed. `version.json` holds `{year, month}`; bump it only for deliberate release-line changes. Every commit on `main` otherwise advances the patch. `node scripts/compute-version.mjs --plain` prints the current version. Do not amend or move release tags once pushed.

## Releases and packages

- GitHub Releases are cut explicitly: push a `v<version>` tag and the `release` workflow verifies the tag matches the computed version, publishes the runtime image, and creates the Release with generated notes.
- GHCR packages publish automatically on every `main` push from the CI `docker-image` job, tagged with the version, release line, commit SHA, and `latest`.
- `latest` always points at the newest `main` push. Intermediate per-commit build tags on GHCR are not releases.

## CI

- Checks: `quality` (lint, prettier, typecheck, tests, contract drift, audits), `secret-scan` (gitleaks), `docker-image` (builds the devcontainer and runtime targets).
- `docker-image` runs on pull requests as well and builds both Docker targets; the GHCR publish and the devcontainer toolchain assertion only run on `main` pushes.
- Never path-allowlist source files in `.gitleaks.toml`. Extend the allowlist with value regexes for test-fixture placeholders (for example `replace-me`). Real secrets never belong in the tree.
- The quality job's lint step is `prettier --check .`; a prettier-dirty committed file breaks CI.

## Conventions

- Contract first: change `api/openapi.yaml`, regenerate the client, keep the `contract:check` gate green.
- For behavior changes, create or update a feature spec in `docs/features/` before implementation starts, and an ADR in `docs/architecture/` for architectural decisions. Templates live in `docs/templates/`.
- Keep privileged filesystem logic on the server; normalize and validate every user-controlled path; prevent path traversal and symlink escapes.
- Security and correctness outrank implementation speed. Small, testable changes preferred.
- Write documentation without AI writing tells (no em dashes, no filler phrasing).
