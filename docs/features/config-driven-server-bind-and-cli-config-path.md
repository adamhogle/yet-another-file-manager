# Config-Driven Server Bind and CLI Config Path

## Summary

Add startup behavior that reads backend listen address and port from the runtime config file, and make config file selection explicit through a CLI parameter with a safe local fallback.

## Problem Statement

The backend currently mixes startup settings between environment variables (`PORT`, `YAFM_CONFIG_PATH`) and config file values (`sharedRoot`, `showHidden`). This creates an inconsistent operational contract and makes it unclear how operators should start the backend in local and containerized environments.

## User Story

As a self-hosting administrator, I want backend startup to use one config file contract for both file-manager behavior and listener settings, so that deployment and local startup are predictable and auditable.

## Scope

- In scope: add `listenAddress` and `listenPort` config fields with defaults.
- In scope: accept config file path as first CLI argument to the Rust backend binary.
- In scope: fallback to `config.yaml` or `config.json` in current working directory when CLI path is omitted.
- In scope: fail startup with a clear error when no CLI path is provided and no CWD fallback file exists.
- In scope: update scripts/docs/tests to match the new startup contract.

## Non-Goals

- Not in scope: browser UI for editing runtime config.
- Not in scope: changing API routes or payload schemas.
- Not in scope: introducing command-line flag parsers or multi-argument option schemas.
- Not in scope: changing authentication, authorization, or share-root security model.

## UX / Flow

1. Operator starts backend with optional config path argument.
2. If a path is provided, backend loads that file.
3. If no path is provided, backend checks `./config.yaml` then `./config.json`.
4. If no config file is found, backend exits with a clear startup error.
5. Backend validates config, initializes shared root settings, then binds listener from config values.

## Technical Notes

- Keep privileged filesystem checks in backend modules.
- Runtime config parser supports YAML and JSON.
- `sharedRoot` validation and canonicalization continue to run before serving requests.
- Listener binding uses config values, not environment variables.
- Config initialization happens at startup and is reused by request handlers.

## Security Considerations

- Continue rejecting invalid `sharedRoot` and non-directory values.
- Preserve path traversal and root-boundary protections for API requests.
- Avoid leaking absolute host filesystem details in API errors.
- Keep startup failures explicit for operators while returning safe runtime errors to clients.

## Acceptance Criteria

- [x] Backend accepts config path as first CLI argument.
- [x] Backend falls back to `config.yaml` or `config.json` in current working directory when CLI argument is omitted.
- [x] Backend fails startup when neither CLI argument nor CWD fallback config exists.
- [x] Runtime config supports `listenAddress` and `listenPort` with defaults.
- [x] Backend listener binds using config-driven address/port.
- [x] Existing API behavior remains unchanged for directory and download routes.

## Test Plan

- Unit: argument resolution logic in backend startup for explicit path, CWD fallback, and missing-config failure.
- Integration: backend API contract tests with explicit config initialization path.
- End-to-end/manual: generated client integration test starts backend with CLI config path and config-driven listener values.

## Open Questions

- Should we introduce named flags (for example `--config`) in addition to positional config path in a future iteration?
