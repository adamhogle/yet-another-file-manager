# ADR 0005: Group-Based Access Control with Config-Driven Allow/Deny Grants

## Status

Proposed

## Context

ADR-0004 delivered all-or-nothing endpoint gating: every authenticated user sees the
whole shared root, and its follow-up work explicitly lists "per-user permissions
(group claims / role checks)" and "embedding the ID token's subject in the signed
payload" as next steps. Two pressures drive this decision:

- Operators need to limit the files accessible by a given user or group. The
  deployment mounts different host directories in under the single shared root, so
  the visible set must be configurable per group.
- The session cookie carries no identity (payload `v1:<exp>` only, documented as a
  residual risk in ADR-0004), so the backend cannot attribute anything to a user,
  and the UI cannot show who is logged in.

Deployment facts constrain the design, the same facts as ADR-0004: the backend is a
single stateless binary with no server-side session infrastructure; config is fixed
for the process lifetime; the identity provider is authentik, which delivers group
membership in the ID token's `groups` claim once the scope is added to the
application's configuration. Maintenance and auditability are hard requirements: with
multiple groups per user, the mapping must stay easy to reason about, and operators
must be able to verify it against the real filesystem structure without deploying a
guess.

## Decision

1. **Identity claims are embedded in the session cookie (`v2`).** The callback
   captures the ID token's subject, display claims (`preferred_username` falling
   back to `name`, then `email`) and the `groups` claim after ID-token validation,
   and the session payload version bumps to `v2` so outstanding `v1` cookies
   re-login. This closes ADR-0004's residual risk, enables per-request audit
   attribution later, and gives `GET /api/v1/users/me` (a contract endpoint,
   identity only) its data with no provider contact. The signing key derivation and
   the 12h TTL are unchanged: embedding identity changes what is signed, not how.
   The cookie stays the single source of truth; per-request userinfo calls would add
   a provider round trip and a server-side dependency the stateless deployment does
   not have.
2. **Group membership comes from the ID token's `groups` claim, filtered to the
   groups the config names.** The config's group names are the single source of
   truth; a user's claim is filtered to configured groups and non-matching groups
   are ignored entirely. There is no prefix scheme and no reserved group name. The
   `groups` claim arrives in the same flow the callback already completes, needs no
   extra network calls or stored credentials, and matches authentik's standard
   behavior.
3. **Grants are config-driven (YAML), the binary stays stateless.** The `access`
   config block is required at startup (fail closed, matching the mandatory-oidc
   precedent of ADR-0004: a silent upgrade must not change what users can see). An
   admin UI would add stateful infrastructure plus an admin authorization model to
   design; "edit YAML and restart" is acceptable for a self-hosted tool.
4. **Two-list allow/deny semantics with set evaluation.** The visible root is the
   union of the global allow list (baseline for every user) and the allow paths of
   the user's groups, inside the shared root, minus every matching global deny and
   every allow-specific deny within its allow's scope. Order does not matter (set
   semantics, no last-match-wins), no negation operator exists, and a denied path
   hides everything underneath it. Allows are plain paths (exposing a subtree means
   naming its parent), so no wildcard ambiguity reaches the allow side. This shape
   was chosen over gitignore's own polarity: gitignore hides what matches, this
   model must hide what does not match (deny-by-default), and copying gitignore's
   `!` plus last-match-wins would import ordering pitfalls the set semantics avoid.
   Deny-by-default is what prevents the leak concern: adding a new directory never
   auto-allows it, it stays hidden until a pattern names it. The root itself is
   browsable for a user whose visible root is non-empty (the global allow list or any
   of their groups' allow entries); only its entries are filtered. A user with no
   allow entries at all gets 404 on the root listing, which the SPA maps to the
   explicit no-access state.
5. **Deny patterns use gitignore glob semantics and match files and directories.**
   `*` matches within one path segment, `?` matches one character, `**` across
   segments, a bare name (no slash, no glob) matches that file or directory name at
   any depth. A pattern containing a
   `/` is anchored to the shared root and matches a prefix of the path's segments
   starting at the first segment, so a denied directory hides everything underneath
   it; `**` follows gitignore (zero or more segments as a middle segment, one or more
   as a trailing segment, so `a/**` hides everything inside `a`, not `a` itself).
   Denies must reach files (`.env`) and directories (`.git`) anywhere under an allowed
   path. Denies are enforced identically on listing and download: a denied file is
   hidden from listings and answers 404 to a download by exact name, so the
   `showHidden` mistake (a listing filter that is not access control) does not repeat
   itself.
6. **Allow-specific denies are paired with their allow entry.** A grant's allow
   entries are objects (`path`, optional `deny`), so the deny is explicitly scoped
   to its allow at the point of use: for example `*.nfo` denied under `/tv-shows`
   while other paths keep `.nfo` visible. No "which deny goes with which allow"
   puzzle when a grant has multiple allows.
7. **Enforcement is evaluated once per request in the gate middleware.** After
   session verification, the middleware computes the request path's access
   decision from the cookie's claims and the config, answers 404 for hidden
   paths, and stores the access context (the scenario's groups and the config)
   in request extensions; the download handler never re-evaluates (the gate is
   the enforcement point) and the listing handler filters each entry with the
   same pure evaluation function read from the extension context. One evaluation
   source means the discipline cannot drift between endpoints, and the
   evaluation is a pure function testable like `gate_decision`.
8. **Hidden paths answer 404.** A request path outside the visible root (listing a
   hidden folder, downloading a hidden file by name) is indistinguishable from a
   nonexistent path: no existence leak, aligned with "cannot be seen". The SPA maps
   a 404 on the root listing to an explicit no-access state.
9. **Verification via `--check-access`, a one-shot startup mode.** The server starts
   with the full production config (including the `oidc` block; iterating rules
   needs no authentik contact since discovery is lazy), walks the real shared root
   for a comma-separated `--groups` scenario, prints the report and exits. The
   report lists each allowed directory once, the denied folders under it (everything
   underneath marked blocked), the individually blocked files, and the rule that
   caused each decision (global deny, or allow-specific deny under which allow, from
   which grant). Rules that match nothing are reported plainly, no fuzzy or
   similarity reasoning: the filesystem is case-sensitive and the report's job is to
   confirm the rules are correct and expected paths are actually hit. The walk
   tracks rule usage against the paths it evaluates (an allow when its coverage
   covers one, a deny when its pattern matches one) and lists the rules that
   matched nothing in the scenario at the end of the report.
10. **Claims staleness is accepted.** Groups are frozen for the life of the session;
    a group change in authentik takes effect at the next login. This matches the
    session-TTL trade-off already accepted in ADR-0004; the escape hatch if it
    proves too stale is a claims refresh or a shorter TTL, both follow-up work.

## Alternatives Considered

1. A whoami endpoint calling authentik's userinfo per request (or caching briefly)
2. A server-side grant store (SQLite or JSON) plus an admin UI
3. Intersection semantics (a path is visible only if all of the user's groups allow it)
4. Cross-group deny-wins (a path any of the user's groups denies is hidden for that user)
5. Gitignore polarity with `!` negation and last-match-wins ordering
6. Full path evaluation allowing a child of a denied parent
7. A group prefix scheme (`groupPrefix: "yafm-"`) filtering the claim by name
8. A standalone CLI verifier and a batch cases-file mode
9. Case-insensitive "did you mean" hints in the check report

A userinfo endpoint adds a provider round trip per identity read and a server-side
dependency. A grant store adds stateful infrastructure and an admin authorization
model. Intersection breaks the baseline: the baseline plus any specific group would
show nothing, and adding a user to a group would shrink their view. Cross-group
deny-wins conflicts with the baseline the same way. Gitignore's polarity and
last-match-wins import ordering pitfalls and misinterpretation risk (the polarity is
inverted from ours); set semantics with two explicitly named lists avoid both. Full
path evaluation would allow re-including a child of a denied parent; strict "denied
means nothing underneath" is simpler to reason about and restructuring the allow
list is a small cost. A prefix scheme is redundant once the config's group names
filter the claim. A standalone CLI cannot verify the rules against the actual
environment; the batch cases-file mode is deferred until the walk report proves
insufficient. Case-insensitive hints get into "why" reasoning; the filesystem is
case-sensitive and the report stays literal.

## Consequences

Benefits:

- Operators can limit access per user and group with a mapping that is explicit at
  the point of use, and adding a directory never silently widens access
  (deny-by-default).
- The session payload carries an identity, closing ADR-0004's residual risk and
  enabling audit attribution and the user menu.
- The check mode verifies the mapping against the real filesystem structure without
  serving traffic, so "why can't this user see this folder?" is answerable
  mechanically.
- The binary stays stateless; the deployment shape is unchanged.

Trade-offs:

- Group changes take effect at the next login, not mid-session (documented
  staleness).
- The claims ride the signed cookie; browsers cap a cookie at ~4 KB and silently
  drop oversized values. The callback validates the claims fit the budget on the
  name=value pair (the browser limit applies to that part, not the Set-Cookie
  attributes) and logs a warning; the practical limit is documented in the config
  example. Authentik group names are short in practice.
- Denies scattered across grants need the check mode to audit; the global deny list
  centralizes shared exclusions.
- The `access` block is required, so an upgrade without it aborts startup with a
  clear error rather than silently changing what users can see.
- Case sensitivity is literal: a config entry that differs only in case matches
  nothing and is reported as matching nothing.

Follow-up work:

- A batch cases-file mode for the check mode if the walk report proves insufficient.
- Per-request audit logging attributed to the session subject.
- A claims refresh or shorter TTL if the staleness proves too short.
- Back-channel logout handling (front-channel RP-initiated logout only today).
- An admin UI if "edit YAML and restart" proves too heavy.

## Security / Operations Impact

- The trust boundary moves from "authenticated" to "authenticated and authorized":
  the gate middleware is the single evaluation point for the access decision, and
  listing and download enforce identically.
- The session cookie's integrity is the boundary for the group claims: signed with
  the independent signing key (never the client secret), so tampering with the
  claims requires the signing key, and the stateless signed cookie has no minting
  path for an attacker. Rotating the key invalidates every outstanding session.
- Path handling keeps the existing discipline: every user-controlled path is
  normalized and validated, and traversal and symlink escapes are prevented. The
  access decision operates on the validated request path inside the shared root.
- 404 for hidden paths prevents existence probing; denied and nonexistent are
  indistinguishable.
- Listings never leak the config shape: no grant lists, no deny patterns in
  responses.
- Observability: the check mode's report is the operator-facing view of the mapping;
  claims that exceed the cookie size budget are logged with a warning at login; the
  gate logs rejected requests as today.
- Rollout: the `access` block is required via config, so an upgrade without it
  aborts startup with a clear error. The `groups` scope must be added to the
  authentik application's configuration for group-based grants to apply; without it
  every user sees only the global baseline, which operators must configure
  consciously.
