# Wiki

Durable knowledge base for Yet Another File Manager. Each page records verified
findings or decisions that outlive a single session. Planning artifacts that
predate the wiki live under `.pi/plans/` (untracked legacy artifacts; `.pi/`
stays as-is — the wiki is the new home going forward).

## Pages

| Page                                                                                         | Summary                                                                                                                                                                                                           | Status |
| -------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| [Frontend TypeScript + ESLint migration research](research/frontend-typescript-migration.md) | Map of the Vue 3 + Vite frontend for the TypeScript and ESLint migration: App.vue anatomy, the generated-client TS-or-JS question, tooling interop with the prettier gate, API types, 9p constraints, docs drift. | Active |
| [Plan: Frontend TypeScript + ESLint migration](plans/frontend-typescript.md)                 | Living plan page for the frontend TypeScript + zod migration and ESLint verification: locked decisions, ISC, architecture, and the strictly sequential todo list (T1–T7). Draws from the research page above.     | Done   |

## Prior Runs (legacy artifacts)

- [Repo health: prioritized fix plan](../.pi/plans/2026-09-04-repo-health/plan.md) — 2026-09-04 repo-health sweep (23 audit issues fixed in 6 waves). Explicitly parked the frontend TypeScript migration (plan.md:49) and corrected docs to say "lint + build" instead of a typecheck — this wiki's migration research un-parks that decision. Full scout context at [.pi/plans/2026-09-04-repo-health/scout-context.md](../.pi/plans/2026-09-04-repo-health/scout-context.md).
- [Plan: OIDC authentication with authentik](../.pi/plans/2026-09-06-oidc/plan.md) — 2026-09-06 OIDC plan: all-or-nothing server-side auth gate on every endpoint (401 JSON for `/api/*`, 302 for browser navigations), signed session cookies, login/callback/logout endpoints, the explicit gate-enumeration test plus a mock-IdP end-to-end flow test, and the debug-gated `YAFM_DISABLE_AUTH` test backdoor. Feature spec at [docs/features/oidc-authentication.md](../docs/features/oidc-authentication.md), ADR at [docs/architecture/0004-oidc-authentication.md](../docs/architecture/0004-oidc-authentication.md). Full scout context at [.pi/plans/2026-09-06-oidc/scout-context.md](../.pi/plans/2026-09-06-oidc/scout-context.md).
