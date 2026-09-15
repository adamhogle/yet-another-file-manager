# YAFM

A self-hosted web file manager: a Rust backend API, a Vue 3 frontend, and OIDC
authentication against authentik.

## Language

### Identity

**Subject**:
The immutable identity of a logged-in user, taken from the ID token's `sub`
claim and embedded in the session cookie at login. The key all authorization
decisions hang off.
_Avoid_: User ID, username (ambiguous with display name)

**Display name**:
The human-readable label shown in the interface for a logged-in user:
`preferred_username`, falling back to `name`, then `email`.
_Avoid_: Username, full name, profile

**Group membership**:
The set of authentik groups a user belongs to, read from the ID token's
`groups` claim at login. Frozen for the life of the session; changes take
effect at the next login.
_Avoid_: Roles, entitlements, claims (too broad)

### Access

**Grant**:
A config-file entry mapping a configured authentik group to allow entries.
Evaluated against group membership at request time; the user's group claim
is filtered to the groups the config names, so only configured groups
matter.
_Avoid_: Permission, ACL, rule

**Allow list**:
The directory paths a grant makes visible. Plain paths only, no wildcards:
exposing a subtree means naming its parent. The allow lists of all of a
user's groups are unioned into their visible scope.
_Avoid_: Permission, ACL, whitelist

**Deny list**:
Patterns that remove paths from visibility within an allow's scope. Denies
win absolutely: once a path is denied, nothing under it is accessible.
Patterns match files and directories at any depth using gitignore-style
wildcards, so no negation operator is needed.
_Avoid_: Filter (the `showHidden` listing filter is not access control),
ignore (gitignore's polarity is inverted from ours)

**Global allow list**:
The baseline access every user gets regardless of group membership, held at
the top level of the access config block. A user whose groups match no
grant still sees this.
_Avoid_: Default group, global grant, everyone (no reserved key)

**Global deny list**:
Deny patterns held beside the global allow list, applying to every user and
every allow path. Where shared exclusions live (.env, .git).

**Allow-specific deny**:
Deny patterns scoped to a single allow entry, applying only under that
allow's path. Where group-local exclusions live (for example .nfo files in a
tv-shows folder that must stay visible elsewhere).

**Access check**:
The server's special startup mode that walks the real shared root and
prints which rules apply to which paths, so an operator can verify the
mapping against the actual structure without guessing.
_Avoid_: Dry run, lint

**Visible root**:
What a given user may browse: the global allow list plus the allow entries
of their groups, inside the shared root, minus every matching deny.
_Avoid_: Shared root (that is the physical filesystem root, not an access
scope)
