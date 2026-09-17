# Feature Spec: User Session Identity

## Status

Proposed

## Summary

The session cookie gains an identity payload and the UI shows the logged-in user in
the upper-right-hand corner with a logout action. The backend exposes
`GET /api/v1/users/me` through the OpenAPI contract, and the SPA redirects to login
with the current path when a request hits an expired session.

## Problem Statement

ADR-0004's all-or-nothing gate authenticates requests but the session payload carries
no identity (documented as a residual risk, "no user identity in the session
payload"). The UI therefore cannot show who is logged in, and per-request audit
logging has nothing to attribute. The user-facing gap: no way to see the account or
log out from the interface. The foundation gap: the follow-up work listed in ADR-0004,
per-user permissions via group claims, needs an identity the backend can read from
the cookie.

## User Story

As a logged-in user, I want to see my account name in the upper-right-hand corner and
log out from there, so that I always know which account I am using and can end my
session without leaving the page.

As an operator, I want the session cookie to carry the authenticated subject, so that
access decisions and audit logging can be attributed to an identity (ADR-0004
follow-up work).

## Scope

- In scope: session cookie payload version `v2`, embedding the ID token's subject
  plus display claims (`preferred_username`, falling back to `name`, then `email`)
  and the groups claim, captured at the callback. The groups claim is consumed by
  `docs/features/group-based-access-control.md`; it rides the same payload so no
  second version bump is needed.
- In scope: `GET /api/v1/users/me` in `api/openapi.yaml`, regenerated client,
  returning identity only.
- In scope: a user menu in the upper-right-hand corner of the SPA shell: display name
  and a logout action wired to the existing `GET /api/v1/auth/logout` browser flow.
- In scope: SPA auto-recovery on session expiry: a 401 from any `/api/*` request
  redirects the browser to `/api/v1/auth/login?returnTo=<current path>`.
- In scope: the payload version bump invalidates outstanding `v1` cookies so old
  sessions re-login instead of failing to parse.

## Non-Goals

- Not in scope: returning grants or visible roots from `/users/me`; identity only
  (the listing endpoints enforce access, and the config shape must not leak).
- Not in scope: user management or profile editing (the identity provider owns the
  account).
- Not in scope: per-user revocation or back-channel logout (unchanged residuals from
  ADR-0004).
- Not in scope: refresh tokens / `offline_access` (re-login through the authentik SSO
  session is an instant redirect).
- Not in scope: hot-reloading config.

## UX / Flow

Happy path:

1. The user logs in through the existing OIDC flow. The callback mints the `v2`
   session cookie carrying subject, display claims and groups.
2. The SPA shell renders the user menu in the upper-right-hand corner: the display
   name, with a logout action.
3. `GET /api/v1/auth/logout` clears the session cookie and redirects to authentik's
   end-session endpoint; the user is returned to the login flow.

Session expiry mid-use:

1. The 12-hour session expires while a tab is open. The next `/api/*` request gets a
   401 JSON error.
2. The SPA reacts by redirecting the browser to
   `/api/v1/auth/login?returnTo=<current path>`; re-login through authentik's SSO
   session is a single redirect, and the user lands back on the path they were on.

Failure modes:

- A `v1` session cookie (pre-upgrade) fails the payload version check and is treated
  as unauthenticated: the browser re-logins, no panic, no error surfaced to the user.
- `users/me` without a valid session: 401, same as every other `/api/*` endpoint.
- A user whose claims carry no display claim at all: the menu falls back to the
  subject value.
- authentik unavailable: unchanged from ADR-0004 (provider-unavailable 503 on the
  flow endpoints).

## Technical Notes

- Cookie payload: `<version>:<expiration>:<subject>:<display name>:<email>:<groups>`.
  Fields that can contain the separator are percent-encoded so values round-trip
  exactly (the same discipline the login cookie's `returnTo` field uses). The version
  constant bumps from `v1` to `v2`; the session TTL (12 hours) and the signing key
  derivation are unchanged.
- Claims capture happens once, at the callback, after ID-token validation: subject
  from the ID token's `sub`, display name from `preferred_username` falling back to
  `name` then `email`, groups from the `groups` claim. The requested scopes stay
  `[openid, profile, email]` plus `groups` added in authentik's application
  configuration; nothing new is requested by YAFM beyond `groups`.
- `GET /api/v1/users/me` returns `{ subject, displayName, email }`; the endpoint reads
  the claims out of the session cookie, no provider contact. It joins the contract
  (`npm run openapi:generate`, `npm run client:generate`, gate `npm run
contract:check`).
- The user menu is a frontend component in `frontend/src/`, rendered by the SPA shell
  (`App.vue` today); logout is a browser navigation to the existing logout endpoint,
  not a generated-client call.
- Error recovery: a 404 on a non-root listing (a directory moved or deleted while
  it is open, or a stale URL) renders an alert-styled error panel with a
  `Back to root` action that navigates to the root and replaces the stale path in
  the URL, so a reload does not land in the error again. The no-access state stays
  a neutral notice panel without the error styling.
- The 401 auto-recovery rides the wrapper client's existing error path (the
  probe-on-error behavior described in `docs/features/oidc-authentication.md`),
  extended to redirect to login with the current path as `returnTo`.
- Cookie size: browsers cap a cookie at ~4 KB and silently drop oversized Set-Cookie
  values. The callback validates that the claims fit the budget (measured on the
  name=value pair, since the browser limit applies to that part and not the
  Set-Cookie attributes) and logs a warning when a user's claims exceed it; the
  practical limit is documented in the config example and ADR-0005.

## Security Considerations

- The session cookie stays signed with the independent signing key (never the OIDC
  client secret), so embedding identity and groups in the payload changes what is
  signed, not how; forging a subject or groups set requires the signing key.
- The payload carries identity and group names only, no secrets; the debug output of
  the cookie jar stays redacted.
- `users/me` reads the cookie's claims; it performs no provider call and exposes
  nothing beyond the identity fields.
- Logout CSRF remains a documented residual risk (unchanged from ADR-0004).
- The display name comes from the identity provider, not user input; the menu renders
  it as text (no HTML injection path).

## Acceptance Criteria

- [ ] The callback mints a `v2` session cookie carrying subject, display claims and
      the groups claim after ID-token validation.
- [ ] `GET /api/v1/users/me` returns the identity fields for a valid session and 401
      without one.
- [ ] The SPA shows the display name in the upper-right-hand corner, verified by
      rendering at desktop, tablet and phone widths (1440, 768, 390 and 320 px)
      against a server serving real data.
- [ ] The logout action navigates to `GET /api/v1/auth/logout`, which clears the
      session and redirects to authentik's end-session endpoint.
- [ ] A 401 from an `/api/*` request redirects the browser to login with the current
      path as `returnTo`.
- [ ] Outstanding `v1` cookies are rejected as unauthenticated (re-login), not
      parsed.
- [ ] Claims that exceed the cookie size budget are logged with a warning at login.

## Test Plan

- Unit: cookie mint/verify round trips for the `v2` payload (valid, tampered
  signature, expired expiry, missing fields, percent-encoded values containing the
  separator); `users/me` handler with and without claims.
- Integration: the generated-client spec covers `users/me` against a live server.
- End-to-end/manual: render the shell with the user menu at 1440, 768, 390 and 320 px
  and check the menu state at each width; exercise login, 401 recovery and logout
  against a real authentik instance.

## Open Questions

- None. The design was settled in a planning session; the access model this feature
  feeds is specified in `docs/features/group-based-access-control.md` and ADR-0005.
