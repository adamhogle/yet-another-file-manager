# Research: Frontend TypeScript + ESLint migration

**Status:** Active (consumed by the plan page)
**Date:** 2026-09-05
**Repo:** /workspaces/yet-another-file-manager
**Used by:** [wiki/plans/frontend-typescript.md](../plans/frontend-typescript.md) (the plan page — decisions recorded there as they were made)
**Prior work:** the 2026-09-04 repo-health run (internal planning directory, not in the public repo) — that plan explicitly parked this migration; see [Parked decision](#parked-decision-un-parked-here).

All facts below are verified against the working tree at SHA-ish `2026-09-05`
state unless marked otherwise.

## Current state (verified)

- Frontend is Vue 3.5.34 + vite 8.2.2 + @vitejs/plugin-vue 6.0.6, ESM
  (`frontend/package.json`). Vite 8 is rolldown-based (`@rolldown/*` bindings in
  the lockfile) — transpile-only, no typechecking in `vite build`.
- Zero `.ts`/`.tsx` files anywhere in `frontend/`. No `tsconfig.json`. No ESLint
  config (`eslint.config.js` absent). No `test` script in `frontend/package.json`.
- Frontend sources: `frontend/src/main.js` (4 lines), `frontend/src/App.vue`
  (375 lines: `<script setup>` at lines 1–73, `<template>` from 75),
  `frontend/src/lib/format.js` (45 lines), `frontend/src/lib/api/client.js`
  (20 lines), `frontend/src/lib/api/generated/client.js` (115 lines, generated).
- `frontend/package.json` scripts: `dev`/`build`/`preview`; `build` =
  `vite build && node ../scripts/copy-public.mjs` (the `copyPublicDir: false`
  9p workaround is baked into `frontend/vite.config.js`).
- Root `package.json`: `lint` = `prettier --check .` (CI gate), `check` =
  `frontend:build && backend:check` (no typecheck anywhere),
  `test` = `frontend:build && backend:test && test:integration`. Root
  devDeps: prettier ^3.8.1, vitest ^4.1.3; runtime dep: yaml ^2.9.0.
- Tests live at root `tests/` (not `frontend/`):
  `versioning.spec.js`, `frontend-helpers.spec.js`,
  `generated-client.integration.spec.js` — all JS. The integration spec spawns
  the real backend and parses the OS-assigned port from stdout.
- Root `vitest.config.js`: node env, include already covers
  `tests/**/*.{test,spec}.{js,ts}` — TS tests are picked up with no config
  change; vitest transpiles TS through its own vite pipeline, no extra dep.
- `.prettierrc`: 2-space indent, singleQuote true, no trailing commas,
  printWidth 100. `.prettierignore`: package locks, `/static/`, and internal/untracked directories,
  `backend/` (rustfmt-owned). Everything else — including a new `wiki/` — is
  prettier scope.

## Parked decision (un-parked here)

The repo-health plan (internal planning directory, not in the public repo) lists under Out of Scope:
"TypeScript migration of the frontend (parked; docs will stop claiming a
typecheck exists)" — repeated as a parked open question at plan.md:161. The
repo-health run's T17 fix corrected `docs/features/` to say "no frontend
typecheck — the frontend is JavaScript-only; TypeScript migration is parked as
backlog" (`docs/features/rust-vue-migration-contract-first.md:76`). This
migration un-parks that decision; every doc listed in [Docs drift](#docs-drift-this-migration-creates)
goes stale again once a typecheck exists and must be updated in the same change.

## App.vue `<script setup>` anatomy (lines 1–73)

Imports:

- `computed, onMounted, ref` from `'vue'`
- `buildDownloadHref, fetchDirectory, toChildPath` from `'./lib/api/client'`
  (extensionless — vite resolves `.js`)
- `formatSize, formatModified` from `'./lib/format.js'` (extensioned)

Mixed extension conventions in one file. Under TS with
`moduleResolution: "bundler"` both forms resolve to `.ts`/`.js` neighbors; under
`"nodenext"` extensionless relative imports fail. Bundler is the natural choice
for a vite app.

State (all inside the SFC, nothing exported — components cannot be unit-tested
as-written; only the extracted `frontend/src/lib` helpers are):

- Consts: `title`, `repositoryUrl`, `currentYear`.
- Refs: `currentPath = ref('')`, `entries = ref([])` (untyped — needs
  `ref<DirectoryEntry[]>([])`), `errorMessage = ref('')`, `isLoading = ref(true)`.
- `breadcrumbs` computed: splits `currentPath` into `{ label, path }` items.

Logic that needs typing:

- `readPathFromUrl()` — `URLSearchParams.get('p') ?? ''` (string).
- `updateUrl(path)` — `history.pushState` set/delete of `p`.
- `loadDirectory(path, updateHistory = true)` — async fetch flow with
  try/catch/finally. Error path narrows with
  `error instanceof Error ? error.message : 'The shared directory is unavailable.'`
  (the catch variable is `unknown` under `useUnknownInCatchVariables`/strict).
- `openDirectory(path)` — delegates to `loadDirectory(path, true)`.
- `onMounted` — initial load without history update + a `popstate` listener
  that is never removed (fine for the root component; prior audit A22 notes
  there is also no fetch AbortController — stale-response race).

No `defineProps`/`defineEmits` — the component takes no props, so the classic
prop-type gotchas do not apply. Typing work is: ref generics, function
signatures, event handler params, and the `unknown` error narrowing.

Vue-specific gotchas:

- Plain `tsc --noEmit` cannot parse `import App from './App.vue'` — SFC
  typechecking requires `vue-tsc` (or a global `*.vue` shim declaration plus
  exclusions, which weakens the check). With vue-tsc +
  `moduleResolution: "bundler"` no shim is needed.
- Adding `lang="ts"` to the script block changes nothing for `vite build` —
  @vitejs/plugin-vue transpiles `lang="ts"` scripts without a tsconfig. The
  build stays green regardless; typechecking is a separate command.

## Generated client + generator (the central design question)

`scripts/generate-openapi-client.mjs` (114 lines, hand-rolled — no
openapi-typescript dependency):

- Parses `api/openapi.yaml` with the `yaml` package; keys `specPath`/
  `outputPath` off `process.cwd()` (documented scratch-run trick: run from a
  temp dir containing a copy of `api/openapi.yaml`).
- Emits one fetch function per operation from a template literal:
  `toFunctionName` derives names from `operationId` (non-alphanumerics
  stripped) with a duplicate-name check (a `Map`, throws
  `Duplicate generated function name '<name>' (…)`).
- `{param}` URL interpolation emits `${encodeURIComponent(params.<name>)}`;
  templated paths gain a `params = {}` argument after `baseUrl`. The current
  API has **no path params** (all query `p`), so all three emitted functions
  use the 3-arg signature `(baseUrl = DEFAULT_BASE_URL, query = {}, init = {})`.
- `/api/v1/download` returns `response.blob()`; the others `response.json()`.
- Error path parses the JSON body and surfaces `body.message` (non-empty
  string), else falls back to
  `Request failed: METHOD <path> -> <status>` — this is how the backend's
  `PublicErrorResponse.message` reaches the UI.
- Header emits `DEFAULT_BASE_URL = ''` (same-origin).

### Pipeline fact (verified by probe)

The committed `client.js` equals **generator raw output + prettier run with the
repo `.prettierrc`**. The raw template's fetch-call lines exceed printWidth 100;
prettier rewraps them. A default-config prettier (double quotes, trailing
commas) produces a spurious diff — prettier must run WITH the repo config
(confirmed in the repo-health run's T9 notes; the config is discovered by
walk-up from the file, so cwd does not matter — the probe only failed because
/tmp has no `.prettierrc`).

The chain: `contract:generate` → `client:generate` =
`node scripts/generate-openapi-client.mjs && npx prettier --write
frontend/src/lib/api/generated/client.js` → `contract:check` =
`contract:generate && git diff --exit-code -- api/openapi.yaml
frontend/src/lib/api/generated/client.js`. Regeneration is idempotent
(byte-identical second run, per T6 notes).

### Option (a) — generator emits TS

Facts the design depends on:

- The render template must emit typed signatures/returns and a header of
  exported types (`DEFAULT_BASE_URL`, request-shape types, `Promise<…>`
  returns). Types come from the OpenAPI schemas ([API types](#api-types)) or
  are emitted inline.
- **Drift check touches two paths:** `client:generate`'s prettier argument and
  `contract:check`'s `git diff` path must change from `client.js` to `client.ts`.
- Prettier formats TS with **zero new deps**: prettier 3.8.3 ships
  `./plugins/typescript` (verified in its package exports).
- **Importers break on rename:** the integration spec imports
  `../frontend/src/lib/api/generated/client.js` by literal path
  (`tests/generated-client.integration.spec.js:13`). A `.js` spec importing a
  `.ts` module resolves through vitest's vite resolver (extensionless or `.ts`
  both work), but the literal `.js` path does not. The wrapper
  `frontend/src/lib/api/client.js` (or `.ts`) imports
  `'./generated/client'` extensionless — fine after rename under bundler
  resolution.
- `frontend/README.md` documents the `client.js` path — needs updating.

### Option (b) — generated client stays JS, consumed from TS

Facts:

- Needs either a hand-written `frontend/src/lib/api/generated/client.d.ts`
  kept in sync manually (the drift check covers only `client.js` — nothing
  verifies the `.d.ts` against the generator, so endpoint changes can silently
  stale the types), or JSDoc `typedef`/`import()` types emitted by the
  generator into the JS itself.
- The generator is unchanged forever; `client:generate`/`contract:check` keep
  their current paths; tests keep importing `.js`.
- The hand-written wrapper and App.vue would still be typed (as `.ts` or via
  JSDoc) — option (b) only pins the generated file.

### Shared dependencies of both options

- The five OpenAPI schema shapes ([API types](#api-types)).
- The prettier-in-pipeline fact above.
- A `tsconfig.json` for typechecking ([Tooling integration](#tooling-integration)).
- The `.github/instructions/*.md` `applyTo` globs ([Docs drift](#docs-drift-this-migration-creates)).

## Tooling integration

Registry facts (checked live):

- eslint latest is **10.10.0**; the 9.x line is also still published. The task
  framing said eslint 9 flat config — eslint 10 is flat-config-only; either
  major works, pin deliberately.
- typescript-eslint 8.69.0 peers: eslint `^8.57.0 || ^9.0.0 || ^10.0.0`,
  typescript `>=4.8.4 <6.1.0`. **TypeScript 7.0.2 is the npm `latest` tag (the
  native Go port) and is NOT supported by typescript-eslint 8.x** — pin TS 5.x
  (latest 5.9.3).
- eslint-plugin-vue 10.10.0 peers: eslint `^8.57.0 || ^9.0.0 || ^10.0.0`,
  `@typescript-eslint/parser` `^7.0.0 || ^8.0.0`,
  `vue-eslint-parser` `^10.3.0` (a peer — install it explicitly),
  `@stylistic/eslint-plugin` optional.
- eslint-config-prettier 10.1.8 disables eslint's formatting rules so eslint
  does not fight the prettier gate (`npm run lint`).
- vue-tsc 3.3.11 peers typescript `>=5.0.0`; depends on
  `@volar/typescript` 2.4.28 + `@vue/language-core` 3.3.11.

A `frontend/tsconfig.json` does not exist. Suggested shape (planner decides):
`strict`, `moduleResolution: "bundler"`, ES2022+ target/lib, include `src`,
`noEmit`. `vue-tsc --noEmit` is the SFC-aware typecheck command; `tsc --noEmit`
covers `.ts` only and cannot resolve `./App.vue` without a shim.

Script changes (candidates, planner decides):

- `frontend/package.json`: add `typecheck` (vue-tsc --noEmit) and/or `lint`
  (eslint). On 9p these fail when invoked as npm scripts — see
  [Env constraints](#env-constraints) — so the 9p-safe node-path forms must be
  documented in CONTRIBUTING.md.
- Root `package.json`: `lint` = `prettier --check .` stays the prettier gate;
  eslint can be added as a separate `frontend:lint`/`frontend:typecheck` root
  script wired into `check`, or folded into `lint`. `check` currently has no
  typecheck; CI quality-job steps are listed below.

CI quality job (`.github/workflows/ci.yml`), current order: Checkout
(fetch-depth 0) → Setup Node → Resolve build version → Rust setup/cache →
Clippy → Format check → Install dependencies (root `npm ci`) → Install frontend
dependencies (`npm ci --prefix frontend`) → Build frontend assets for contract
checks → Check OpenAPI and generated client drift (`contract:check`) → Backend
and frontend checks (`npm run check`) → **Lint (`npm run lint` =
prettier --check .)** → Dependency advisories → npm audit → Tests → Build.
CI runs on a normal filesystem: `npx` and bare binaries work there; eslint
would slot in as a step after (or alongside) Lint, or as part of `check`.

Vitest: root `vitest.config.js` already includes `.ts` in the include glob —
no config change needed. Tests live at root `tests/` and import frontend
sources by relative path (`../frontend/src/lib/format.js`); if those sources
become `.ts` the import specifiers change, resolved fine by vitest's vite
pipeline.

## API types

From `api/openapi.yaml` components and the backend DTOs
(`backend/src/lib.rs:35–68`, all `#[serde(rename_all = "camelCase")]` except
`EntryKind` which is `lowercase`):

- `EntryKind` = `'directory' | 'file'`.
- `DirectoryEntry`: required `name`, `kind`; `sizeBytes` is int64 ≥ 0 nullable
  (`type: [integer, 'null']`), `modifiedAt` is string nullable — both are NOT
  in `required`, so the TS shape is `sizeBytes?: number | null` /
  `modifiedAt?: string | null` (optional-or-null).
- `DirectoryListing`: required `currentPath`, `entries`; `parentPath` nullable,
  not required.
- `HealthResponse`: required `status`, `service`.
- `PublicErrorResponse`: required `message`.

Current consumption is untyped: the generated client returns
`response.json()` (any). App.vue reads `listing.currentPath`/`entries` and
`entry.kind/name/sizeBytes/modifiedAt`; `formatSize`/`formatModified` already
handle `null`/`undefined` at runtime.

**Type-quirk gotcha:** `tests/frontend-helpers.spec.js` asserts
`formatSize(undefined)` returns `'NaN KB'`. Typing the parameter as
`number | null` makes that test call a type error; the typing must accommodate
`undefined` or the test must change. Same consideration for the JSX-style
comment in `format.js` documenting the null-handling rationale.

## Env constraints

The 9p drvfs mount (chmod fails EPERM) — verified workarounds, documented in
CONTRIBUTING.md's "9p Checkouts" section:

- `npm ci --no-bin-links` and `npm ci --no-bin-links --prefix frontend`.
  `frontend/node_modules/.bin` is **empty** (verified) — all binaries must be
  invoked via `node node_modules/<pkg>/bin/…` paths or npm run scripts that
  work without .bin links (they don't: `npm run build` calls bare `vite`,
  which is why CONTRIBUTING documents the node-path form).
- vite via `node node_modules/vite/bin/vite.js build`; prettier via
  `node node_modules/prettier/bin/prettier.cjs` (`npx prettier` fails on 9p —
  per the repo-health run's T7/T9 findings; the root `client:generate` and
  `openapi:generate` scripts embed `npx prettier` and fail locally on 9p but
  work in CI); vitest via `node node_modules/vitest/vitest.mjs`.
- New dev deps (typescript, vue-tsc, eslint, eslint-plugin-vue,
  vue-eslint-parser, typescript-eslint, eslint-config-prettier) install into
  `frontend/node_modules` with `--no-bin-links` and must be invoked via node
  paths on 9p.
- `frontend/node_modules` today: vue, vite 8.2.2 (rolldown-based),
  @vitejs/plugin-vue — 26 top-level entries; everything above is new.
- Root `.npmrc` is `engine-strict=true` with **no** bin-links key — deliberate
  (a repo-wide bin-links key would break CI's bare-binary npm run scripts).
- Devcontainer recommends `Vue.volar` + `esbenp.prettier-vscode`
  (`.devcontainer/devcontainer.json`) — already right for TS in `.vue` files.

## Docs drift this migration creates

Every doc that mentions the frontend tooling; items 1–3 are the ones that go
**silently** stale (unapplied instructions / false statements):

1. `docs/features/rust-vue-migration-contract-first.md:76` — "no frontend
   typecheck — the frontend is JavaScript-only; TypeScript migration is parked
   as backlog" (the T17 correction). Becomes false. Its Alternatives list
   (line 27) also treats "stricter TypeScript contracts" as a rejected
   SvelteKit-era alternative — historical, update only if the ADR is amended.
2. `.github/instructions/backend-frontend-architecture.instructions.md` —
   `applyTo: 'backend/src/**/*.rs,frontend/src/**/*.{js,vue},frontend/vite.config.js,package.json'`
   — the `{js,vue}` glob stops matching migrated `.ts` files, so the
   architecture guidance silently stops applying. Needs `{js,ts,vue}` (or
   `{ts,vue}`).
3. `.github/instructions/testing-quality.instructions.md` —
   `applyTo: 'backend/**/*.rs,frontend/src/**/*.{js,vue},tests/**/*.{test,spec}.{js,ts},backend/tests/**/*.rs'`
   — same `{js,vue}` drift (its tests glob already includes `ts`).
4. `README.md` Scripts table — `npm run lint # Prettier validation`; new
   typecheck/lint entries need adding; the lint description needs rewording if
   lint grows eslint.
5. `CONTRIBUTING.md` — Engineering Standards gate list (`npm run check`,
   `lint`, `test`, `build`) and the 9p Checkouts list (4 verified forms) — a
   5th form is needed for any new binary-invoking script.
6. `.github/copilot-instructions.md:27–28` — generic "type checks" language and
   the quality-gates list; stale only if aggregate script definitions change.
7. `.github/agents/feature-delivery.agent.md:24` — same generic gates.
8. `.github/pull_request_template.md:20` — `npm run lint` checkbox (generic).
9. `docs/features/file-download.md:84`, `docs/features/secure-directory-listing.md:101`,
   `docs/features/public-release-license-and-footer.md:41` — Validation
   Evidence lists (generic; stale only if aggregate script definitions change).
10. `docs/architecture/0002-rust-backend-vue-frontend-contract-first.md` —
    UX/Flow "stable, typed API interactions" language becomes true with TS;
    Alternatives section is historical.
11. `frontend/README.md` — documents the generated client at
    `src/lib/api/generated/client.js` and the regeneration flow; the path
    changes under option (a).

## Gotchas for the planner

1. **The generated client is prettier-formatted generator output.** Any
   generator rewrite must keep the prettier step inside `client:generate` in
   sync (the argument path changes if the output becomes `client.ts`), and
   prettier must run with the repo config — default-config prettier emits
   double quotes and a spurious diff (verified by probe).
2. **TypeScript 7.0.2 is npm `latest` but unsupported by typescript-eslint
   8.x** (peer range `>=4.8.4 <6.1.0`). Pin TS 5.x (5.9.3 latest) unless
   typescript-eslint ships TS 7 support.
3. **eslint-plugin-vue requires `vue-eslint-parser` as a peer** — it is not
   pulled in transitively by typescript-eslint; install it explicitly.
4. **`.bin` is empty on 9p.** New npm scripts that invoke binaries fail
   locally; the CONTRIBUTING 9p list needs the node-path forms for vue-tsc and
   eslint (verify each bin path after install — e.g. `vue-tsc/bin/vue-tsc.js`).
5. **`formatSize(undefined)` → `'NaN KB'` is asserted in tests.** Type the
   parameter to accommodate `undefined` or change the test.
6. **Mixed import-extension conventions in App.vue** (extensionless
   `'./lib/api/client'` vs extensioned `'./lib/format.js'`). Both work under
   `moduleResolution: "bundler"`; extensionless fails under `"nodenext"`.
   Bundler is the natural choice with vite.
7. **Plain `tsc` cannot typecheck `.vue` SFC imports** — use vue-tsc for the
   full check, or keep `.ts`-only checking with `tsc --noEmit` (then
   `main.ts`'s `./App.vue` import needs a shim or exclusion).
8. **The integration spec imports the generated client by literal path** —
   update `tests/generated-client.integration.spec.js:13` if the file is
   renamed to `.ts`.
9. **Vitest at root owns `tests/**`**; frontend has no test script — don't
   move tests into `frontend/`silently, and don't forget`vitest.config.js`'s glob already covers `.ts`.
10. **`contract:check` also diffs `api/openapi.yaml`** — its `openapi:generate`
    script's `npx prettier` on 9p fails locally (pre-existing A16 quirk,
    unrelated to this migration); only the client side is affected here.
11. **`.gitattributes` is `* text=auto eol=lf`** — new `.ts`/`.vue` files land
    LF automatically; no renormalization needed this time (the repo-health
    Wave 0 fix already landed).
