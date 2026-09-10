# Plan: Frontend TypeScript + ESLint migration

**Status:** Done
**Date:** 2026-09-05
**Repo:** /workspaces/yet-another-file-manager
**Draws from:** [Frontend TypeScript + ESLint migration research](../research/frontend-typescript-migration.md) — the scout's findings page (current state, App.vue anatomy, tooling registry facts, verified prettier-in-pipeline probe, docs drift, planner gotchas). The 2026-09-04 repo-health run at `.pi/plans/2026-09-04-repo-health/` (legacy, untracked) parked this migration (plan.md:49, 161) and documents repo standards (prettier CI gate, spec-first feature work, ADRs for architecture changes, generated files are AUTO-GENERATED DO NOT EDIT).
**Completed:** 2026-09-05 — every ISC verified at the T7 gate (check/lint/tests/contract green in-session, contract chain idempotent, CI workflow untouched); T1–T6 landed through `c9aea74`, plus the T7 flip.

## Intent

Un-park the frontend TypeScript migration the 2026-09-04 repo-health run deliberately deferred: convert the frontend from pure JavaScript to TypeScript, add ESLint verification, and make the contract-first story real — the generated client emits zod schemas that validate the runtime API contract at the request boundary. Every doc that mentions frontend tooling stays truthful in the same change; CI-plan text uses plain standard commands with the 9p workarounds documented separately.

## User Story

As a maintainer of a contract-first file manager, I want the frontend typed with a typecheck that actually runs and the runtime contract validated, so that endpoint changes surface as type errors or loud runtime failures instead of silently rendering wrong data.

## Locked decisions (rationale inline)

1. **Generated client emits TypeScript: zod schemas + `z.infer` types + typed fetch functions** — user decision: "the contract cannot be assumed, we should use zod to validate it on the frontend". Compile-time interfaces alone don't validate the runtime shape; the OpenAPI-declared schema is enforced where the response enters the UI. Zero new formatting deps (prettier 3.8.x ships its TypeScript plugin — the `client:generate` prettier step just changes its argument path). Zero hand-written contract surface (no unverified `.d.ts`).
2. **zod schemas are generator-emitted from `api/openapi.yaml` `components.schemas`** — user decision: "generator emits them". Single source of truth: the contract YAML drives schemas, types, and functions; `contract:check` covers all of it; schemas regenerate with the contract. The 5 shapes are flat objects + one string enum, so emission is tractable (~+100 lines of generator logic). Verified live: zod 4.5.4 is npm `latest` (`zod/v4`, `zod/mini` subpaths exist; no peer/engine constraints clashing with the TS pin).
3. **Typecheck scope: `vue-tsc --noEmit` covering `.vue` + `.ts`, extended to root `tests/`** — plain `tsc --noEmit` is weaker and needs a shim for `./App.vue`; vue-tsc needs no shim under `moduleResolution: "bundler"`. One root tsconfig covers `frontend/src` and `tests/` in a single command. The vite build stays green regardless (vite transpiles `lang="ts"` scripts without a tsconfig).
4. **Strictness: tsconfig `strict: true`** + typescript-eslint `recommended` (non-type-aware) + vue `recommended` + eslint-config-prettier. Full `strict type-checked` linting needs `parserOptions.project` wiring and slower lint runs; vue-tsc already catches the type errors that matter in a ~400-line codebase.
5. **Tests migrate to TypeScript as well** — user decision. Root `tests/*.spec.js` → `.ts`; root vitest owns them and `vitest.config.js`'s include glob already covers `.ts` (no config change). vitest transpiles TS through its own vite pipeline, no extra dep. `@types/node` 22.x is already at root (transitive from vitest) for the specs' `node:*` imports.
6. **CI placement: both wired into existing gates — zero CI workflow changes.** Root `check` gains `typecheck` (a correctness gate riding the existing "Backend and frontend checks" CI step); root `lint` grows eslint alongside prettier (both static analysis, one CI step). The existing install steps (`npm ci` root, `npm ci --prefix frontend`) already cover every new dependency.
7. **Configs + TS dev deps live at the ROOT npm workspace; frontend owns only the runtime `zod` dep.** The root package is not the backend (that is Rust under `backend/`) — it is the shared tooling home that already owns prettier (lints frontend sources), vitest (tests import frontend sources), and the contract scripts (touch the generated client). One tsconfig + one eslint config cover both areas; the frontend build needs none of the TS tooling.
8. **App.vue stays one component, typed in place** — consistent with the prior run's no-restructure decision (helper extraction only). Docs drift items 1–3 (the silent ones) fixed in the same change, plus every doc that becomes directly false from the decisions above.

## Behavior

### Happy Path (target end state)

1. `npm run check` = `frontend:build && typecheck && backend:check` — the standard quality gate now includes full SFC typechecking.
2. `npm run lint` = `prettier --check . && eslint "frontend/src/**/*.{ts,vue}" "tests/**/*.ts"` — prettier stays the formatting gate; eslint adds code-quality verification. CI's Lint step runs the updated aggregate script unchanged.
3. `npm run contract:generate` regenerates `frontend/src/lib/api/generated/client.ts` — zod schemas, `z.infer` types, typed fetch functions — prettier-formatted with the repo config; `contract:check` diffs `api/openapi.yaml` + `client.ts` and stays green.
4. App.vue consumes a typed, validated `DirectoryListing`; a backend shape mismatch throws at the parse boundary and the UI shows the error message.
5. The root tests (now `.ts`) pass under vitest; `vitest.config.js` needs no change.

### Edge Cases & Error Handling

- **Nullable vs optional:** `sizeBytes`/`modifiedAt`/`parentPath` are optional-or-null per the YAML (in `type: [T, 'null']`, NOT in `required`) → `z.number().int().min(0).nullable().optional()` etc. Mirroring exactly matters — the backend sends both `null` and absent; a wrong mapping rejects valid responses (the integration spec catches it at the parse).
- **Error path:** the generated function validates the error body through `PublicErrorResponseSchema` and surfaces `.message` only when non-empty (the current guard); non-conforming body → fallback `Request failed: METHOD <path> -> <status>`.
- **`formatSize(undefined)` → `'NaN KB'`** stays asserted in `tests/frontend-helpers.spec.ts`; the param is typed `number | null | undefined` to accommodate it (the runtime NaN path already produces the asserted output).
- **Strict-mode narrowing:** `loadDirectory`'s catch variable is `unknown` — the existing `error instanceof Error ? error.message : …` pattern works unchanged.
- **Download:** `downloadFile()` returns `Promise<Blob>` — binary stream, no schema, no parse.
- **9p checkouts:** new dev deps install with `--no-bin-links` (root `node_modules` for the TS/eslint tooling, `frontend/node_modules` for zod); bin paths verified after install; `npx` fails locally on 9p but works in CI — CONTRIBUTING documents the node-path forms.

## Scope

### In Scope

- Frontend sources → TS: `main.js` → `main.ts`, `format.js` → `format.ts`, `api/client.js` → `api/client.ts`, `App.vue` script block `lang="ts"` typed in place
- Generator emits zod schemas + typed functions into `frontend/src/lib/api/generated/client.ts`; `client:generate`/`contract:check` paths updated
- Root `tsconfig.json` + root `eslint.config.js` (new); root dev deps: typescript ~5.9.3, vue-tsc, eslint ^10, eslint-plugin-vue ^10, vue-eslint-parser, @vue/eslint-config-typescript ^14, eslint-config-prettier ^10; frontend runtime dep: zod ^4.5.4
- Root `tests/*.spec.js` → `.ts` with updated import specifiers
- Root `check`/`lint` absorb typecheck + eslint (zero CI workflow changes)
- Docs drift sweep: all 11 research-page items (3 silent) that go stale, in the same change

### Out of Scope

- Restructuring App.vue into multiple components (typed in place)
- Changing the OpenAPI contract shape (paths/response schemas stay as-is)
- ESLint on `scripts/*.mjs`, `frontend/vite.config.js`, `vitest.config.js` (possible follow-up)
- Migrating `scripts/*.mjs` to TS (root scripts stay JS; `allowJs` in the tsconfig covers the spec imports)
- Any backend change beyond the contract chain's client regeneration
- Parallel workers; amending landed commits (T1–T17 through `8b6bca6`)

## Effort & Quality

- **Level:** production (robust, typechecked, docs truthful)
- **Tests:** existing vitest suite migrates to TS; no new unit tests strictly required — the integration spec exercises the zod validation implicitly against the real backend (a shape mismatch fails at the parse)
- **Docs:** inline updates to the drift items + this wiki plan page (living doc: draft → active → done)

## Constraints

- **Prettier is a CI gate** (2-space, single quotes, no trailing commas, printWidth 100); all edits pass `npm run lint`. The generated client is prettier-formatted generator output — the prettier step stays inside `client:generate` and must run WITH the repo config (default-config prettier emits double quotes and a spurious diff — verified by probe in the repo-health run).
- **Generated files:** `frontend/src/lib/api/generated/client.ts` keeps its `AUTO-GENERATED FILE. DO NOT EDIT.` header; changes go through `npm run contract:generate`; drift checked via `git diff --exit-code`.
- **TS version pin:** typescript ~5.9.3 — TS 7.0.2 is npm `latest` (the native Go port) and is NOT supported by typescript-eslint 8.x (peer `>=4.8.4 <6.1.0`).
- **Feature work requires spec first; ADR for architecture changes.** This is a tooling migration — the wiki plan page + research page are the recorded rationale; no new ADR needed (ADR 0002's "typed API interactions" language becomes true).
- **Workers run strictly sequentially in the same git repo, never parallel; each worker commits alone.** Never amend the landed commits.
- **CI-plan text uses plain standard commands** (`npm run …`, `npx eslint …`, `cd frontend && npx vue-tsc --noEmit`) since CI runs on a normal filesystem. The 9p workarounds (`npm ci --no-bin-links`, node-path invocations like `node node_modules/vite/bin/vite.js`, `node node_modules/prettier/bin/prettier.cjs`, `node node_modules/vitest/vitest.mjs`) stay documented separately in CONTRIBUTING for in-session verification only. New dev deps install with `--no-bin-links`; their bin paths must be verified after install.

## Ideal State Criteria

### Core Functionality

- [x] ISC-1: `frontend/src` contains zero `.js` sources; `App.vue` script block is `lang="ts"`, typed in place
- [x] ISC-2: generator emits zod schemas + `z.infer` types + typed fetch functions into `frontend/src/lib/api/generated/client.ts` from `api/openapi.yaml`
- [x] ISC-3: each generated fetch function validates its JSON response through its zod schema before returning
- [x] ISC-4: `vue-tsc --noEmit` passes over `frontend/src` (incl. `.vue`) and `tests/` under a strict tsconfig
- [x] ISC-5: eslint passes over `frontend/src` (incl. `.vue`) and `tests/` — typescript-eslint recommended + vue recommended + eslint-config-prettier
- [x] ISC-6: `contract:generate` / `contract:check` work end-to-end with `client.ts` (prettier in-pipeline, idempotent regeneration)
- [x] ISC-7: root tests migrated to `.ts` and pass under vitest (glob already covers `.ts`)
- [x] ISC-8: root `check` includes typecheck, root `lint` includes eslint, CI quality job green with zero workflow changes

### Edge Cases

- [x] ISC-9: `formatSize(undefined)` → `'NaN KB'` quirk resolved deliberately (typing accommodates `undefined`, test unchanged)
- [x] ISC-10: docs drift items 1–3 (silent) fixed — instructions `applyTo` globs match `.ts`, the T17 "JS-only" correction goes true again — plus every directly-stale doc updated in the same change
- [x] ISC-11: CONTRIBUTING documents 9p node-path forms for the new binaries (bin paths verified after install)

### Anti-Criteria

- [x] ISC-A-1: No hand-written contract surface that can silently drift (no unverified `.d.ts`)
- [x] ISC-A-2: Generated client keeps its AUTO-GENERATED header and regeneration workflow
- [x] ISC-A-3: App.vue not restructured into components — typed in place
- [x] ISC-A-4: No `any`-casts to silence the typechecker where a real type exists

## Approach

Generator-owned zod schemas + typed fetch functions; one root tsconfig typechecked by vue-tsc over `frontend/src` + `tests/`; eslint wired via `@vue/eslint-config-typescript`; tests migrate to TS; the existing `check`/`lint` gates absorb the new checks with zero CI workflow changes. Chosen over the alternatives because: generator-emitted schemas eliminate contract drift entirely (the user's stated objection to compile-time-only typing), a single root tsconfig matches how prettier/vitest already work here, and wiring into existing gates avoids new CI steps while making `npm run check` truthful about the typed frontend.

### Key Decisions

1. **Option (a) generator emits TS** over (b) JS-generated client + hand-written `.d.ts` — (b) has a silent-staleness risk (nothing verifies the `.d.ts` against the generator); (a) makes types live with the code and costs a one-time generator rewrite.
2. **zod runtime validation at the parse boundary** — the generated functions call `Schema.parse(await response.json())`; types flow via `z.infer`. Aligned with ADR 0002's contract-first goal.
3. **vue-tsc over plain tsc** — full SFC check, no shim, one command under `moduleResolution: "bundler"`.
4. **eslint via `@vue/eslint-config-typescript`'s `defineConfigWithVueTs`** over manual parser wiring — verified by probe: `flat/recommended` alone leaves `parserOptions.parser` null (espree would choke on TS script syntax); `defineConfigWithVueTs` produces vue-eslint-parser as the main parser for `.vue`, the TS parser as the script-block delegate, and TS rules scoped to `**/*.vue` too.
5. **Tests → TS at root** — vitest's glob already covers `.ts`; specs keep importing frontend sources and the generated client.
6. **Zero CI workflow changes** — the aggregate `check`/`lint` scripts absorb the new checks; the existing install steps cover the new deps.

### Architecture

Two-workspace layout preserved; root owns repo-wide quality tooling (precedent: prettier + vitest already live at root and touch frontend sources).

| File                                               | Change                                                                                                                                                                        |
| -------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `frontend/src/main.js` → `.ts`                     | trivial rename (4 lines)                                                                                                                                                      |
| `frontend/src/App.vue`                             | script block `lang="ts"` typed in place: `ref<DirectoryEntry[]>([])`, typed signatures, `unknown` catch narrowing; template untouched                                         |
| `frontend/src/lib/format.js` → `.ts`               | typed params (`number \| null \| undefined` where the runtime handles it)                                                                                                     |
| `frontend/src/lib/api/client.js` → `.ts`           | wrapper: delegates to the generated client, re-exports inferred types for App.vue                                                                                             |
| `frontend/src/lib/api/generated/client.js` → `.ts` | generator output: `import { z } from 'zod'`, emitted schemas + types + typed fetch functions that parse their responses; AUTO-GENERATED header stays                          |
| `scripts/generate-openapi-client.mjs`              | gains schema emission from `components.schemas`; output → `client.ts`; prettier arg + `contract:check` diff paths → `client.ts`                                               |
| `frontend/package.json`                            | `dependencies` gains `zod: ^4.5.4` — scripts unchanged                                                                                                                        |
| root `tsconfig.json` (new)                         | `strict`, `moduleResolution: "bundler"`, ES2022 + DOM lib, `noEmit`, `allowJs` (resolves the specs' `.mjs` script imports), include `frontend/src` (.ts/.vue) + `tests` (.ts) |
| root `eslint.config.js` (new)                      | `defineConfigWithVueTs(pluginVue.configs['flat/recommended'], vueTsConfigs.recommended)` + eslint-config-prettier; lints `frontend/src/**/*.{ts,vue}` + `tests/**/*.ts`       |
| root `package.json`                                | devDeps + `typecheck` script; `check`/`lint` absorb the new checks                                                                                                            |
| `tests/*.spec.js` → `.ts`                          | import specifiers updated; integration spec imports the new `client.ts` path                                                                                                  |

**Verified tooling facts** (checked live during planning): eslint 10.10.0 latest; typescript-eslint 8.69.0 (peers eslint `^8.57.0 || ^9 || ^10`, typescript `>=4.8.4 <6.1.0` — TS 7.0.2 NOT supported); eslint-plugin-vue 10.10.0 exposes `configs['flat/recommended']`; vue-eslint-parser 10.4.1; @vue/eslint-config-typescript 14.9.0 (deps pull typescript-eslint + vue-eslint-parser transitively — still install them explicitly for clarity); eslint-config-prettier 10.1.8 exposes `/flat`; vue-tsc 3.3.11 (bin `bin/vue-tsc.js`, peers typescript `>=5.0.0`); typescript `bin/tsc`; eslint `bin/eslint.js`; zod 4.5.4.

### Data Flow

1. `contract:generate` → `openapi:generate` (unchanged) → `client:generate` = `node scripts/generate-openapi-client.mjs && npx prettier --write frontend/src/lib/api/generated/client.ts` → `contract:check` = regenerate + `git diff --exit-code -- api/openapi.yaml frontend/src/lib/api/generated/client.ts`. Prettier runs with the repo config (walk-up discovery); regeneration stays idempotent.
2. Generated `getDirectory()` fetches → `DirectoryListingSchema.parse(await response.json())` — the OpenAPI-declared shape enforced at the boundary; a mismatch throws instead of handing `any` to the UI. Types flow to App.vue via the wrapper's re-exports (`DirectoryEntry`, `DirectoryListing` = `z.infer<…>`).
3. Error path: `PublicErrorResponseSchema.parse(body)` → `.message` surfaced only when non-empty; non-conforming body → fallback string.
4. `downloadFile()` returns `Promise<Blob>` — no schema, no parse.
5. Tests: the integration spec imports the generated client directly — zod validation exercised against the real backend; a backend shape mismatch fails loudly at the parse.

## Dependencies

- Root devDeps (new): typescript ~5.9.3 (pinned; 7.0.2 unsupported), vue-tsc ^3.3, eslint ^10, eslint-plugin-vue ^10, vue-eslint-parser, @vue/eslint-config-typescript ^14, eslint-config-prettier ^10
- Frontend runtime dep: zod ^4.5.4 (ships in the bundle; tree-shakeable, small core)
- `@types/node` 22.x already at root (transitive from vitest) — covers the tests' `node:*` imports; no explicit install
- No new Rust deps; no CI workflow changes

## Risks & Open Questions

From the premortem (assumed the plan failed, worked backwards):

- **Risk (mitigated):** the generator rewrite's schema emission mismatches the backend's actual shape (nullable without optional) → the integration spec's real-backend test fails loudly at the parse; mirror the YAML exactly.
- **Risk (mitigated):** the emitted `client.ts` is not prettier-stable/idempotent → prettier runs with the repo config inside `client:generate`; byte-identical double regeneration verified before committing.
- **Risk (mitigated):** TS 7.0.2 npm-latest trap → pin typescript ~5.9.3 (verified peers).
- **Risk (early-verifiable):** vite 8 (rolldown) transpiles `lang="ts"` without a tsconfig → the App.vue todo verifies the build immediately after the change.
- **Risk (early-verifiable):** vue-tsc + root tsconfig resolves `vue`/`zod` types and covers `.vue` SFC imports → the first typecheck todo verifies before anything downstream commits.
- **Risk (early-verifiable):** 9p bin paths for the new binaries unknown until install → verify after install, document node-path forms in CONTRIBUTING (ISC-11).
- **Risk (mitigated):** silent docs drift (instructions `applyTo` globs, the T17 correction) → fixed in the same change (ISC-10).
- **Risk (accepted):** zod adds bundle weight — small, tree-shakeable core.
- **Risk (accepted):** `noUncheckedIndexedAccess` not enabled (strict only) — `formatSize`'s units indexing stays as-is; a possible follow-up.
- **Open question parked:** none — all six judgment calls + the zod architecture were decided with the user.

## Review (2026-09-05 reviewer run)

**Verdict:** APPROVED — no blockers; status stays **Done**.

Independent review of the full change set (`c560656..HEAD`, 15 commits) along both axes — correctness and spec conformance — with the complete verification suite re-run in-session. Every critical claim in the locked decisions and the todo status notes was re-verified with concrete evidence.

### Verification evidence (in-session, 9p node-path forms)

- `node node_modules/vue-tsc/bin/vue-tsc.js --noEmit` — green, exit 0 (strict root tsconfig over `.vue` + `.ts`, `tests/` included).
- `node node_modules/prettier/bin/prettier.cjs --check .` — green, all matched files use the repo code style.
- `node node_modules/eslint/bin/eslint.js --no-error-on-unmatched-pattern "frontend/src/**/*.{ts,vue}" "tests/**/*.ts"` — green, exit 0; also verified WITHOUT the flag (exit 0 — the flag is inert since T5).
- Frontend build (`cd frontend && node node_modules/vite/bin/vite.js build` + `node ../scripts/copy-public.mjs`) — green (969ms; zod ships in the 145.21 kB bundle, 47.81 kB gzip).
- Backend: `cargo check`, `cargo clippy -- -D warnings`, `cargo fmt --check` all green; `cargo test` 31/31 across 9 suites.
- Root vitest: 16/16 — 14 unit + 2 integration against the real backend; the zod parse ran against actual responses and the 404 error path (`..%2Fsecret` → `PublicErrorResponseSchema` → message surfaced) validated at the parse.
- Contract chain: generate + repo-config prettier TWICE — second cycle byte-identical (sha1 `6f1b54a1…` both cycles) — then `git diff --exit-code -- api/openapi.yaml frontend/src/lib/api/generated/client.ts` green. The `openapi:generate` side was also re-run and is idempotent (no drift on `api/openapi.yaml`).
- CI workflow: `git log c560656..HEAD -- .github/workflows/` empty and ci.yml byte-identical before/after (sha1sum match); ci.yml runs the aggregate scripts (`npm run check`, `npm run lint`) so the new checks ride the existing steps. ISC-8 confirmed.

### Correctness spot-checks (probed)

- Generator emission maps `api/openapi.yaml` exactly: `sizeBytes` → `z.number().int().min(0).nullable().optional()`, `modifiedAt`/`parentPath` → `.nullable().optional()` (all three in `type: [T, 'null']`, NOT in `required`); `EntryKind` → `z.enum(['directory', 'file'])`; `$ref`/array-of-`$ref` mapping correct; schemas emitted topologically in the sketch order (EntryKind, DirectoryEntry, DirectoryListing, HealthResponse, PublicErrorResponse).
- Response handling keyed off the 200 response's content presence: `getApiV1Download` returns `Promise<Blob>` via `response.blob()` with its error path parsed; `getApiV1Directory`/`getApiV1Health` parse their JSON responses — the `includes('/download')` hack is gone.
- Error path: `PublicErrorResponseSchema.parse(body)` inside the existing try, `.message` surfaced only when non-empty, fallback `Request failed: METHOD <path> -> <status>` otherwise; zod object default (unknown keys stripped, non-string message → throw → caught) keeps the old guard's semantics.
- Duplicate-name check present (`Duplicate generated function name '<name>' (…)`); AUTO-GENERATED header + `Source:` comment intact at `client.ts:1-2`.
- Emitted `client.ts` typechecks under the strict root tsconfig (vue-tsc green) and lints clean.
- `formatSize`'s `const bytes = sizeBytes ?? NaN;` body adjustment: runtime-identical across a 19-case probe of the old vs new implementations (0 mismatches; `formatSize(undefined)` → `'NaN KB'` in both — `??` coalesces only null/undefined and null is already guarded, so `undefined → NaN` is exactly the old arithmetic path). All test assertions byte-unchanged (`tests/frontend-helpers.spec.ts`).
- zod parse boundary: a wrong enum value or non-int `sizeBytes` throws loudly (`instanceof Error` → App.vue's catch displays the message); the nullable+optional combos the backend actually sends (null and absent) are all accepted.
- App.vue: `<script setup lang="ts">` typed in place (`ref<DirectoryEntry[]>([])`, typed signatures, `unknown` catch narrowing), zero `any`-casts, popstate listener kept, template byte-unchanged (diff shows script-block hunks only).
- The `vue/block-lang` claim behind the T1/T4 `scriptLangs` deviations confirmed in the installed library: `allowNoLang: scriptLangs.includes('js')` at `@vue/eslint-config-typescript/dist/index.cjs:203`.
- Docs drift sweep accurate: zero "JavaScript-only"/"no frontend typecheck" hits under `docs/` and `.github/`; both `applyTo` globs match `.ts`; zero `client.js` references in markdown; T6 items 6-9 verified no-change-needed (all name still-valid script names with no false wording).
- zod 4.5.4 is a frontend runtime dep (`frontend/package.json` `dependencies`); frontend scripts unchanged; `@types/node` resolved transitively at root.
- CI workflow genuinely unchanged (byte-identical, zero commits touching `.github/workflows/`).

### Findings

No P0 or P1 findings.

### [P2] Generator topological sort lacks cycle detection

**File:** `scripts/generate-openapi-client.mjs` (`topologicallySortSchemas`)

**Issue:** `visit()` has no in-progress marker, so a schema cycle in `components.schemas` — e.g. a tree node whose `children` array `$ref`s its own type, plausible for a file manager — recurses until `RangeError: Maximum call stack size exceeded` instead of a descriptive error (probed with the exact visit logic replicated from the generator). Additionally, even with detection the emitted code for a cyclic schema would need `z.lazy(() => Schema)` — a plain `const` reference inside `z.array(Schema)` is evaluated during initialization and would throw a TDZ ReferenceError at runtime.

**Suggested Fix:** add a visiting set and throw a descriptive error naming the cycle (matching the generator's existing "Unsupported $ref" error style), or emit `z.lazy` for cyclic refs. The current contract is flat objects + one string enum, the generator exits loudly, and it is run manually — hence P2, not P1.

### Non-blocking observations

1. In-session integration-spec runs require a working `cargo`: the spec spawns cargo with `env: process.env`, and in this devcontainer the pinned-toolchain rustup sync fails with permission-denied unless `RUSTUP_TOOLCHAIN=stable` is set — then `waitForListenPort` polls the full 150s budget and throws "Backend did not report listening", with the real cause only visible in the piped `[backend]` stderr (a failed spawn surfaces no error otherwise). Pre-existing structure: the beforeAll/spawn/wait logic predates this migration (5eada32); the change only typed it. CI runs on a normal filesystem where the plain form works. A cheap future improvement: surface the spawn `error` event or an early exit-code check in `waitForListenPort`.
2. `mediaType.endsWith('json')` in `resolveResponseSchema` is case-sensitive; an uppercase media type in a future YAML would silently map to `Blob`. The YAML is generator-owned (`openapi:generate` writes `application/json`), so speculative — noted for completeness only.

## Triage outcome (2026-09-05)

**Verdict:** APPROVED; the P2 cycle-detection finding fixed in commit `6e97979` (in-session, not by a worker): a visiting set in `topologicallySortSchemas` rejects cyclic schemas with a descriptive error naming the cycle (probed with a temp `openapi.yaml` containing a self-referential schema — the generator now exits 1 with the message instead of `RangeError`); the dead double `emitted.has` check removed in the same edit. `z.lazy` emission for cyclic refs remains a documented future refinement (the current contract is flat plus one enum, so nothing breaks today). The non-blocking observations stay as noted (spawn error-event surfacing is a pre-existing structure improvement, and the case-sensitive `endsWith('json')` is generator-owned and speculative).

Verification after the fix: generate + repo-config prettier twice — byte-identical (`sha1 6f1b54a1…` both cycles, the fix changes nothing for the flat contract); `git diff --exit-code -- api/openapi.yaml frontend/src/lib/api/generated/client.ts` green; `cargo`-free checks all green (eslint, vue-tsc, prettier); the plan page's review section prettier-clean.
