# Feature Spec: Group-Based Access Control

## Status

Proposed

## Summary

Config-driven allow/deny grants limit what each user and group can see inside the
shared root. A server-side enforcement layer evaluates the grants once per request,
listing and download endpoints filter the view identically, and a `--check-access`
startup mode verifies the mapping against the real filesystem structure without
serving traffic.

## Problem Statement

ADR-0004's gate is all-or-nothing: every authenticated user sees the whole shared
root, and the `showHidden` listing filter is explicitly documented as not an access
control. Operators need to limit the files accessible by a given user or group.
The deployment mounts different host directories in under the root, so the visible
set must be configurable per group, enforced on the server for listings and
downloads alike, and verifiable without deploying a guess.

## User Story

As an operator, I want to state which folders on the root each group can see and
which files or folders must stay hidden anywhere, so that users only see the files
meant for them and sensitive data cannot be reached by name.

As a logged-in user, I want the interface to show only the files my groups can see,
so that hidden content is simply not there.

## Scope

- In scope: a required `access` config block: global allow (baseline for every
  user), global deny (every user, everywhere), and grants mapping configured
  authentik groups to allow entries with optional paired allow-specific denies.
- In scope: deny patterns with gitignore glob semantics (`*` within a segment, `**`
  across segments, a bare name matching at any depth), matching files and
  directories. Allows are plain paths, no wildcards.
- In scope: enforcement evaluated once per request in the gate middleware and
  exposed to handlers via request extensions; identical filtering on listing and
  download.
- In scope: hidden paths answer 404 (indistinguishable from nonexistent) for direct
  navigation; the SPA maps a 404 on the root listing to an explicit no-access state.
- In scope: the user's groups claim is filtered to the groups the config names;
  non-matching groups are ignored entirely.
- In scope: the `--check-access` one-shot startup mode walking the real shared root
  for a comma-separated `--groups` scenario, printing allowed directories, denied
  folders (everything underneath marked blocked), individually blocked files and
  rule provenance.
- In scope: example config, README section, ADR-0005.

## Non-Goals

- Not in scope: a per-file ACL engine (directory-level grants cover the realistic
  self-hosted use case).
- Not in scope: a runtime admin UI or server-side grant store (config-driven, the
  binary stays stateless).
- Not in scope: grant changes taking effect without re-login (claims are frozen for
  the session; the staleness is accepted and documented, revisit later if it proves
  a problem).
- Not in scope: hot-reloading the access config.
- Not in scope: multi-root support (different host directories are mounted under the
  single shared root; the model operates on paths under it regardless of mounts).
- Not in scope: case-insensitive or fuzzy matching hints in the check report (the
  filesystem is case-sensitive; a rule that matches nothing is reported as matching
  nothing).

## UX / Flow

Browsing with access:

1. The user logs in; the session carries their groups (see
   `docs/features/user-session-identity.md`).
2. Listings show only the visible root: the global allow plus the allow entries of
   the user's groups, minus every matching deny.
3. Denied files and folders are absent from listings and unreachable by exact name
   (404), not merely hidden from the listing.

No access:

1. A user whose groups match no grant sees only the global baseline. With no global
   baseline configured, the visible root is empty.
2. The root listing answers 404; the SPA renders an explicit no-access state (empty
   listing plus an explanation), never a blank screen that looks like a broken
   server.

Verification:

1. The operator runs the server with `--check-access --groups yafm-tv` (comma
   separated list). The server starts with the real production config, walks the
   real shared root and prints the report, then exits.
2. The report lists each allowed directory once, the denied folders under it (with
   everything underneath marked blocked), and the individually blocked files, each
   with the rule that caused it (global deny, or allow-specific deny under which
   allow, from which grant). Rules that match nothing are reported plainly.

Failure modes:

- Invalid deny patterns abort startup with a clear full-sentence error (fail closed,
  like the rest of the config validation).
- A missing or incomplete `access` block aborts startup, matching the mandatory-oidc
  precedent.
- Group membership changes in authentik take effect at the next login, not
  mid-session (documented staleness).

## Technical Notes

- Config shape:
  ```yaml
  sharedRoot: ../dev-data/share
  oidc: { ... } # unchanged
  access:
    allow: ['/common'] # global baseline: every user sees this
    deny: ['.env', '.git'] # global deny: every user, everywhere
    grants:
      yafm-tv:
        allow:
          - path: '/tv-shows'
            deny: ['*.nfo']
      yafm-devs:
        allow:
          - path: '/dev'
            deny: ['dev/tmp/**']
          - path: '/projects'
  ```
- Evaluation semantics, one sentence: a user's visible root is the union of the
  global allow list and the allow paths of their groups, inside the shared root,
  minus every matching global deny and every allow-specific deny within its allow's
  scope. Order does not matter (set semantics, no last-match-wins), no negation
  operator exists, and a denied path hides everything underneath it. The root
  itself is browsable for a user whose visible root is non-empty (only its entries
  are filtered); a user with no allow entries at all gets 404 on the root listing.
- Deny pattern grammar follows gitignore's glob semantics: `*` matches anything
  except `/` within one segment, `?` matches one character, `**` matches across
  segments, a bare name (no slash, no glob) matches that file or directory name at
  any depth. Allows take no wildcards: exposing a subtree means naming its parent.
- Enforcement: the middleware evaluates the access decision once, after session
  verification, and stores the access context (the scenario's groups and the
  config) in request extensions; the download handler never re-evaluates (the
  gate answers 404 for hidden request paths and is the enforcement point), and
  the listing handler filters each entry with the pure evaluation function read
  from the extension context, so the discipline cannot drift between endpoints.
  The evaluation is a pure function of the claims and the config (testable like
  `gate_decision`). Deny rules hold for the fully-decoded form of a path too:
  when the once-decoded request path differs from the form the handlers open
  through the percent-decode fallback, a deny matching the decoded form wins, so
  a request that only resolves after a second decode cannot bypass the rules.
  The required access block aborts startup when missing (fail closed).
- Hidden paths: a request path outside the visible root (listing a hidden folder,
  downloading a hidden file by name) answers 404 with the existing error shape, so
  hidden and nonexistent are indistinguishable and no existence leaks.
- Groups: read from the ID token's `groups` claim at login, filtered to the groups
  the config names. The config's group names are the single source of truth; there
  is no prefix scheme.
- Check mode: `--check-access` on the backend binary (matching the existing
  CLI-config-path convention) with `--groups` (comma separated). The full production
  config is required, including the `oidc` block; iterating rules needs no
  authentik contact (discovery is lazy, startup makes no network call). The mode
  walks directories fully, calls out files denied by name patterns within each
  directory (the exceptions; everything else inherits the parent verdict), and
  exits after the report.
- Glossary: `CONTEXT.md` defines Subject, Display name, Group membership, Grant,
  Allow list, Deny list, Global allow/deny list, Allow-specific deny, Baseline,
  Access check and Visible root.

## Security Considerations

- Enforcement is server-side at the listing and download paths; every user-controlled
  path is normalized and validated, and traversal and symlink escapes are prevented
  (the same discipline as the existing secure directory listing).
- Denies win absolutely within their scope: a denied folder blocks its contents and a
  denied file blocks the download by exact name. The `showHidden` mistake (a listing
  filter that is not access control) does not repeat itself.
- The session cookie's integrity is the boundary for the group claims: the cookie is
  signed with the independent signing key, so tampering with the claims requires the
  signing key; the stateless signed cookie has no minting path for an attacker.
- Listings filter the view and never leak the config shape (no grant lists, no deny
  patterns in responses).
- 404 for hidden paths prevents existence probing; denied and nonexistent are
  indistinguishable.
- Claims are frozen for the session: a group change in authentik does not affect an
  open session until re-login. This is the accepted staleness trade-off, documented
  for operators.
- The `everyone` grant concept does not exist; the global baseline is explicit config,
  so no reserved group name can be confused with a real authentik group.

## Acceptance Criteria

- [ ] The `access` block is required; a missing or incomplete block aborts startup
      with a clear error.
- [ ] Invalid deny patterns abort startup with a clear error.
- [ ] A user's visible root equals the global allow plus their groups' allow paths
      minus matching denies, asserted by tests over the evaluation function.
- [ ] A denied folder hides everything underneath it; a denied file is hidden from
      listings and answers 404 to a download by exact name.
- [ ] Deny and allow rules are enforced identically on listing and download.
- [ ] A user whose groups match no grant sees only the global baseline; with no
      baseline, the root listing answers 404 and the SPA shows the no-access state,
      verified at desktop, tablet and phone widths (1440, 768, 390 and 320 px).
- [ ] The user's groups claim is filtered to the configured groups.
- [ ] `--check-access --groups <list>` starts with the real config, prints the report
      (allowed directories, denied folders with everything underneath marked blocked,
      blocked files, rule provenance) and exits.
- [ ] Rules that match nothing are reported plainly; no fuzzy or similarity hints.
- [ ] Listing responses never contain grant or deny configuration details.

## Test Plan

- Unit: the glob matcher (`*`, `**`, bare names, case-sensitive, files and
  directories), the evaluation function (union, denies within scope, baseline,
  no-match scenarios), and the middleware request-extension plumbing.
- Integration: listing and download against a live server with grants configured
  (allowed paths, denied files by name, denied subtrees, 404s); the generated-client
  spec stays green.
- End-to-end/manual: `--check-access` against `dev-data` with a two-group scenario,
  verifying the report against the actual directory structure; render the no-access
  and visible states in the SPA at 1440, 768, 390 and 320 px.

## Open Questions

- None. The design was settled in a planning session; the vocabulary is in
  `CONTEXT.md` and the decision record is ADR-0005.
