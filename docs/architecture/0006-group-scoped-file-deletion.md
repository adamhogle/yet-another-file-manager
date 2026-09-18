# ADR 0006: Group-Scoped File Deletion as the First Write Operation

## Status

Accepted (2026-09-17)

## Context

ADR-0005 delivered group-based access control with allow/deny grants, and every
capability YAFM ships today is read-only: listing, download, and identity. Two
pressures drive this decision:

- Users need to remove a file that should no longer be shared. Today the only
  remedy is shell access on the host, which breaks the self-hosted workflow.
- Deletion is destructive and irreversible on a shared root that multiple groups
  see. A global delete switch would make the blast radius of one flag the whole
  visible root, so the grant must be narrow and explicit.

Deployment facts constrain the design, the same facts as ADR-0004 and ADR-0005:
the backend is a single stateless binary; config is fixed for the process
lifetime; the session cookie (v2) carries the group claims; and the documented
deployment mitigation is mounting the shared root read-only. That last fact is
the one this decision changes.

## Decision

1. **Deletion is granted per group, all or nothing, and scoped to that group's
   own allow entries.** Each grant gains an optional `delete` boolean, default
   false. A user may delete a file if and only if the file is visible to them
   under the existing allow/deny evaluation, and an allow entry of one of their
   groups that has `delete: true` covers the file. This shape was chosen over
   scoping to the whole visible root and over a global flag plus per-grant:
   scoping to the grant's own allows keeps the global baseline read-only for
   everyone and bounds the blast radius of a misconfigured grant to that
   grant's allow paths. A per-path delete allow/deny was rejected for the same
   reason ADR-0005 rejected per-path access rules: it adds a second polarity
   puzzle to reason about for little gain.

2. **The contract gains `DELETE /api/v1/file?p=<relative path>`.** The DELETE
   method is not a CORS simple request: a cross-origin attacker would need a
   preflight the backend never approves, and the session cookie is SameSite=Lax
   so a cross-site fetch does not carry it. No extra token is needed. Error
   responses are 400 (invalid request, including a directory target), 403
   (visible but not deletable for this user), 404 (hidden or nonexistent,
   indistinguishable), and 503 (storage unavailable, including an unwritable
   target directory). `DirectoryEntry` gains `canDelete` (boolean, false for
   directories) so the UI can show the action only where it can succeed; a
   session-level capability boolean was rejected because it cannot express the
   per-file scoping (a user with delete in one grant still cannot delete
   baseline files).

3. **Authorization is a new pure function `access::can_delete`.** It reuses
   `is_path_visible` for the visibility half and adds grant-allow coverage for
   the delete half, testable like `evaluate`. The access gate evaluates the
   delete path like the other data endpoints (404 for hidden paths, access
   context stored for the handler), keeping one evaluation source so the
   discipline cannot drift between endpoints.

4. **Target resolution is symlink-safe and never follows a symlink for the final
   component.** The parent directory is canonicalized and verified to be within
   the shared root, and the final name (a single validated segment) is appended
   after that resolution. The entry is classified with `symlink_metadata`: only
   regular files are deleted, directories and symlinks are refused with 400.
   This differs from download, which serves the canonicalized target. With
   `remove_file` on the resolved in-root path, a symlink replaced at the target
   removes the link, not the target, so deletion cannot reach outside the root.

5. **Failure modes map onto the existing shapes with a fail-closed signal.**
   NotFound from `remove_file` becomes 404 (a concurrent delete);
   PermissionDenied becomes 503 with the fixed unwritable-mount message. The
   message names no host path: naming the directory would leak it, and the
   operator knows the shared root from config. That message is
   the fail-closed signal for a read-only mount. There is no startup writability
   check: the OS-level error at delete time is the enforcement, and a startup
   check would duplicate what the kernel already refuses.

6. **The UI confirms before deleting.** File rows where `canDelete` is true show
   a delete action; clicking it enters a two-step inline confirmation in the row
   (no modal dependency). Confirming sends the delete request, reloads the
   listing on 204, and reloads on 404 too since the file is already gone.
   Directories never show a delete action.

## Alternatives Considered

1. Scoping delete to the whole visible root (global baseline included)
2. A global delete flag plus per-grant flags
3. A session-level capability boolean on `users/me` or the listing root
4. Per-path delete allows and denies
5. A startup writability check for delete-allowed scopes
6. Allowing symlink deletion (safe with `remove_file`, but muddies "only files")

Scoping to the whole visible root would let a group delete baseline files, which
widens the destructive surface beyond what the grant names. A global flag adds a
second switch with the same widening problem. A session-level boolean cannot
express the per-file scoping and would over-show the delete action on baseline
files. Per-path delete rules import the ordering and polarity pitfalls ADR-0005
avoided with set semantics. A startup check duplicates the kernel's refusal.
Symlink deletion would be safe but contradicts the "only files" requirement.

## Consequences

Benefits:

- Operators can grant deletion per group with an explicit, narrow scope: the
  grant's own allow paths. The global baseline stays read-only for everyone.
- The blast radius of a misconfigured grant is bounded to that grant's allow
  paths, which is auditable in the config.
- The binary stays stateless; the deployment shape is unchanged apart from the
  mount flag.

Trade-offs:

- The shared root must be writable for delete-allowed directories, which removes
  the documented read-only-mount mitigation. YAFM has no upload path, so users
  cannot plant files themselves, but a planted FIFO blocking a download thread
  and hard-link escapes regain force where the mount is writable.
- Delete is permanent and irreversible; the UI confirmation is the only guard
  against a mis-click.
- The listing classifies a symlink by its target's type, so a symlink to a file
  shows up as a file row whose delete answers 400. Symlinks in the share are
  operator-planted and rare; this is documented rather than fixed with an extra
  stat per listing entry.
- A new HTTP method and a 403 status enter the contract, a small client and
  contract surface change.

Follow-up work:

- A trash or undo mechanism if irreversibility proves too harsh.
- Symlink-aware `canDelete` if the 400-on-symlink UX proves confusing.
- Annotating delete grants in the `--check-access` report if operators need the
  report to audit the destructive capability.
- Per-request audit logging attributed to the session subject (carried over from
  ADR-0005).

## Security / Operations Impact

- The trust boundary moves from "authenticated and authorized (read-only)" to
  "authenticated and authorized (read plus a scoped destructive write)": delete
  authorization never overrides visibility, and deny rules still win.
- Path handling keeps the existing discipline: every user-controlled path is
  normalized and validated, and traversal and symlink escapes are prevented.
  The delete target resolves inside the shared root and never follows a symlink
  for the final component.
- 404 for hidden paths on the delete endpoint prevents existence probing.
- Rollout: operators must mount the shared root writable for delete-allowed
  directories and configure the `delete` flag consciously. The fixed
  unwritable-mount message is the fail-closed signal when the mount flag
  and the config disagree.
