# Secure Directory Listing

## Status

Done (2026-05-22)

## Summary

Introduce the first user-facing file-management capability: listing files and directories from a configured root path through a server-controlled interface.

## Problem Statement

The application currently has no functional file-management behavior. Users need a safe way to browse a shared root directory without exposing arbitrary filesystem access or leaking host details.

## User Story

As a self-hosting administrator using the file manager, I want to browse the contents of the configured shared directory, so that I can verify what files and folders are available to share.

## Scope

- In scope: server-side directory listing for a configured root directory
- In scope: runtime configuration from a JSON or YAML file rather than browser UI state
- In scope: display of immediate child entries for a requested relative path
- In scope: safe handling of empty directories and missing directories within the allowed root
- In scope: user-visible error states for invalid paths and inaccessible locations

## Non-Goals

- Not in scope: upload, download, rename, move, or delete operations
- Not in scope: recursive listing of entire directory trees in one request
- Not in scope: authentication or multi-user access control
- Not in scope: rich media previews or metadata extraction beyond basic file attributes
- Not in scope: any UI for editing or persisting application configuration

## UX / Flow

1. The user opens the file manager home page.
2. The client requests the listing for the root directory.
3. The server returns the current path and its immediate entries.
4. The UI renders a breadcrumb path (`root / ... / current`) where each segment is clickable for direct navigation.
5. The UI renders folders and files with a clear empty state if no entries exist.
6. If the user requests an invalid or blocked path, the UI shows a safe error message without leaking host filesystem details.

## Technical Notes

- Use server-only path validation and directory reads in `backend/src/lib.rs`.
- Load runtime configuration from a JSON or YAML file, with a Docker-friendly default path and optional explicit path override.
- Initialize and validate runtime configuration at server startup, then reuse the initialized config during request handling.
- During startup, resolve configured `sharedRoot` to its canonical realpath and persist that value in memory for request handlers.
- Fail startup when `sharedRoot` does not exist as a directory.
- When configuration is invalid, startup validation fails on the first error it hits and aborts startup with that specific message (for example `Configuration sharedRoot "..." does not exist; create it or fix the config file`); no server logger collects all issues before failing (`load_app_config` in `backend/src/lib.rs`).
- Resolve incoming relative paths against a configured root directory.
- Reject absolute paths, traversal attempts, and symlink escapes.
- Return a typed response model containing the current relative path and a list of entries.
- Keep route handlers thin and place filesystem and validation logic in server utilities.

## Security Considerations

- Normalize and validate all requested paths before filesystem access.
- Enforce Linux/POSIX path semantics and reject Windows-specific separators or forms.
- Prevent path traversal via `..` segments and encoded traversal forms.
- Detect and block symlink resolution outside the configured root.
- Avoid returning absolute host paths in errors or payloads.
- Treat unreadable directories as controlled failures with safe error messaging.

## Acceptance Criteria

- [x] The root directory can be listed through a server-controlled endpoint or page load.
- [x] The shared root is configured from a JSON or YAML file rather than browser-managed settings.
- [x] Only entries inside the configured root directory are returned.
- [x] Requests using absolute paths or traversal attempts are rejected.
- [x] Symlink paths that escape the configured root are rejected.
- [x] Empty directories render a clear empty state.
- [x] Errors do not reveal absolute host filesystem paths.
- [x] Breadcrumb navigation shows `root` and each directory segment as clickable targets.
- [x] Breadcrumb navigation replaces dedicated parent-only navigation controls.

## Test Plan

- Unit: path normalization and root-boundary validation utilities
- Integration: server-side listing behavior for valid paths, empty directories, missing paths, traversal attempts, and symlink escape attempts
- End-to-end/manual: root page load shows entries and safe error states

## Open Questions

- Should hidden files be shown by default or deferred to a later feature flag?

## Completion Notes

- Runtime configuration is loaded from JSON/YAML via server-side configuration loader only.
- Runtime configuration is now validated during server startup instead of during endpoint execution.
- Configured shared roots are canonicalized to realpaths during startup and reused from in-memory config.
- Startup validation fails fast on the first config error with an error naming the problem; there is no collector that logs all issues before aborting.
- Directory browsing uses server-rendered navigation with query parameter `p`.
- Directory listing now returns Name, Size, and Modified metadata for compact details-style display.
- Directory path display now uses clickable breadcrumbs (`root / foo / bar`) in the client and no longer uses a separate "Up" button.
- Path traversal and symlink escape protections are enforced in server path guards and listing service.

## Validation Evidence

- `npm run lint`
- `npm run check`
- `npm run test`
- `npm run build`
