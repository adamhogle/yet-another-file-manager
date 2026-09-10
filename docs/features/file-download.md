# File Download

## Status

Done (2026-05-23)

## Summary

Add a file download action from the directory listing that always serves files as forced attachments.

## Problem Statement

Users can browse files but cannot download them. This blocks the primary file-sharing use case for a file manager.

## User Story

As a person browsing shared files, I want to click a download control next to a file, so that the browser saves the file locally regardless of file type.

## Scope

- In scope: add a per-file download action in the listing UI at the far-right actions area
- In scope: add a server download route that validates paths within configured root
- In scope: force attachment download behavior for all file types
- In scope: user-safe error responses for invalid paths, missing files, and non-file targets

## Non-Goals

- Not in scope: directory archive downloads
- Not in scope: bulk or multi-select downloads
- Not in scope: authentication/authorization changes
- Not in scope: preview behavior changes

## UX / Flow

1. User opens a directory listing page.
2. User clicks the download icon on a file row.
3. Browser sends a GET request to the download route with the selected relative file path.
4. Server validates and resolves the file path within configured root.
5. Server responds with attachment headers to force download.
6. Browser starts saving the file. If validation fails, user gets a controlled error response.

## Technical Notes

- Add a server route at `/api/v1/download` in `backend/src/lib.rs`.
- Keep privileged path resolution in `backend/src/lib.rs` with `validate_relative_path` and `ensure_within_root`.
- Reuse path normalization and root-boundary protections.
- Ensure resolved targets are files (not directories).
- Set `Content-Disposition: attachment` and `Content-Type: application/octet-stream`.
- Add response hardening headers suitable for downloads.

## Security Considerations

- Normalize and validate incoming relative paths.
- Enforce Linux/POSIX path semantics and reject Windows-specific separators or forms.
- Reject traversal attempts and absolute paths.
- Resolve symlinks and reject targets escaping root.
- Avoid exposing host absolute paths in error messages.
- Restrict download behavior to file entries only.

## Acceptance Criteria

- [x] A download icon is visible on file rows in the far-right actions column.
- [x] Clicking the icon starts a browser file download.
- [x] Downloads are always forced as attachments regardless of file content/type.
- [x] Traversal/absolute-path attempts are rejected with safe errors.
- [x] Symlink escapes outside configured root are rejected.
- [x] Directory targets are rejected.
- [x] Tests cover valid download resolution plus invalid and escape paths.

## Test Plan

- Unit: file download path resolution for valid files, invalid paths, missing files, directories, symlink escape attempts
- Integration: download route response headers and error mapping
- End-to-end/manual: clicking download icon from listing starts attachment download

## Open Questions

- Should failed downloads route back to a UI error panel instead of returning route-level HTTP errors?

## Validation Evidence

- `npm run test`
- `npm run check`
- `npm run lint`
- `npm run build`
