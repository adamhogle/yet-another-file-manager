# Group-Scoped File Deletion

## Status

Done (2026-09-17)

## Summary

Add the first write operation to YAFM: deleting regular files. The capability is granted per group, all or nothing, and only reaches files the user can already see.

## Problem Statement

Users can browse and download shared files but cannot remove a file that should no longer be shared. Today the only remedy is shell access on the host, which breaks the self-hosted workflow. Deletion must stay bounded: a destructive capability on a shared root needs an explicit, narrow grant instead of a global switch.

## User Story

As a member of a group with the delete grant, I want to delete a visible file from the file manager, so that I can remove files that should no longer be shared without host access.

## Scope

- In scope: server-side deletion of regular files within the shared root
- In scope: a per-grant `delete` boolean in the access config
- In scope: delete authorization that follows the existing allow/deny visibility rules
- In scope: a `canDelete` flag per directory entry so the UI can show the action only where it can succeed
- In scope: a confirmation step in the UI before the delete request is sent

## Non-Goals

- Not in scope: deleting directories or symlinks
- Not in scope: uploading, creating, renaming, or moving files
- Not in scope: a trash folder, undo, or soft-delete mechanism
- Not in scope: a global (non-group) delete flag
- Not in scope: per-path delete allows or denies

## UX / Flow

1. The user opens a directory listing. File rows where `canDelete` is true show a delete action beside the download action, separated by a 2px inline margin so the two actions do not read as one control.
2. Clicking the delete action opens a native modal `<dialog>`: the title names the file ("Delete <name>?"), the body states that the removal is permanent, and the buttons are Confirm and Cancel. `showModal()` traps focus inside, Escape closes, a backdrop click cancels, and focus lands on Cancel so Enter does not destroy.
3. Confirming closes the dialog and sends the delete request. On 204 the listing reloads and the file is gone. On 404 the listing also reloads, since the file is already gone.
4. Any other error (400, 403, 503) surfaces through the existing error panel without reloading.
5. Directories never show a delete action.

## Technical Notes

- Config shape: each grant gains an optional `delete` boolean, default false. Files visible only through the global allow baseline are never deletable, since delete authorization counts only grant allow entries.
- Authorization rule: a user may delete a file if and only if the file is visible to them under the existing allow/deny evaluation, and an allow entry of one of their groups that has `delete: true` covers the file. New pure function `access::can_delete(access, groups, relative) -> bool`, testable like `evaluate`.
- Contract: `DELETE /api/v1/file?p=<relative path>` returning 204 No Content. Error responses: 400 (invalid request, including a directory target), 403 (visible but not deletable for this user), 404 (hidden or nonexistent, indistinguishable), 503 (storage unavailable, including an unwritable target directory). `DirectoryEntry` gains `canDelete` (boolean, false for directories).
- The access gate evaluates the delete path like the other data endpoints: a hidden path answers 404 and the gate stores the access context for the handler.
- Handler order: validate path, 404 for hidden, 403 when `can_delete` is false (a pure in-memory authorization check that runs before any filesystem access), then the filesystem resolution, 400 for a non-regular-file target (classified during the target resolution), then remove.
- Target resolution is symlink-safe: the parent directory is canonicalized and verified to be within the shared root, and the final name (a single validated segment) is appended after that resolution. The entry is classified with `symlink_metadata` (no symlink following), so `remove_file` cannot reach outside the root. This differs from download, which serves the canonicalized target.
- `remove_file` errors map onto the existing shapes: NotFound becomes 404 (a concurrent delete), PermissionDenied becomes 503 with the fixed unwritable-mount message. The message names no host path: naming the directory would leak it, and the operator knows the shared root from config. That message is the fail-closed signal for a read-only mount; there is no startup writability check, the OS-level error at delete time is the enforcement.
- The test-only Disabled state (no access context) allows delete, matching the existing pre-access behavior of the test binaries; production always has the access block.
- CSRF: the DELETE method is not a CORS simple request, so a cross-origin attacker would need a preflight the backend never approves, and the session cookie is SameSite=Lax so a cross-site fetch does not carry it. No extra token needed.

## Security Considerations

- Every user-controlled path is normalized and validated before filesystem access; traversal and symlink escapes are prevented.
- The target must be a regular file: directories and symlinks are refused with 400.
- Delete authorization never overrides visibility: deny rules still win, and a file hidden by any deny is not deletable even when a delete-enabled grant's allow covers it.
- 404 for hidden paths prevents existence probing on the delete endpoint.
- The blast radius of a misconfigured grant is bounded to that grant's own allow paths; a `delete: true` on a broad allow is a destructive operator decision and is visible in the config, not in the API.
- Delete is permanent and irreversible; the UI confirmation is the only guard against a mis-click.

## Acceptance Criteria

- [x] A group with `delete: true` can delete a regular file visible via that grant's allow entries.
- [x] A group without the delete grant gets 403 on a visible file.
- [x] Files visible only through the global allow baseline are not deletable by anyone.
- [x] A file hidden by a deny rule answers 404 to a delete request, indistinguishable from nonexistent.
- [x] Deleting a directory or a symlink target is refused with 400.
- [x] The UI shows the delete action only on file rows where `canDelete` is true, behind a modal confirmation.
- [x] A successful delete removes the file and the listing reflects it after reload.
- [x] Errors do not reveal absolute host filesystem paths.

## Test Plan

- Unit: `access::can_delete` semantics (grant coverage, baseline exclusion, deny wins, no permission), config validation of the `delete` flag, delete handler behavior (file only, hidden 404, 403, directory 400, symlink refusal, race 404), gate evaluation of the delete path.
- Integration: delete against a live server with grants configured, including the generated-client integration spec.
- End-to-end/manual: UI verified at 1440, 768, 390 and 320 px with full-page screenshots, delete action shown and hidden per row, the modal open with focus on Cancel and the gap between the two actions, post-delete refreshed listing.

## Open Questions

- None.

## Completion Notes

- The per-grant `delete` boolean defaults to false; only grant allow entries count, so the global baseline is never deletable.
- `DELETE /api/v1/file` returns 204 on success, 400 for a non-regular-file target, 403 for a visible file the user may not delete, 404 for hidden paths, and 503 with the fixed unwritable-mount message when the shared root is read-only. The message names no host path.
- Target resolution is symlink-safe: the parent directory is canonicalized and verified to be within the shared root, and the entry is classified with `symlink_metadata`, so `remove_file` never follows a symlink for the final component.
- The generated client returns `Promise<void>` for the 204; the frontend shows the delete action behind a native modal `<dialog>` with Confirm and Cancel, and the two row actions carry a 2px inline margin.

## Revision History

- 2026-09-17: accepted and implemented as the inline two-step row confirmation.
- 2026-09-19: the confirmation became a native modal `<dialog>` (Confirm/Cancel) after deployment feedback: the inline row confirmation crammed three small controls into one cell, and the modal gives the decision its own space with labeled buttons, focus and an Escape path. The two row actions gained a 2px margin.
