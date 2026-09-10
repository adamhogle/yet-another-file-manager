# Todos: Frontend TypeScript + ESLint migration

**Plan:** [frontend-typescript.md](frontend-typescript.md) — read it first; it holds the locked decisions, ISC, and rationale.
**Tag:** `frontend-typescript`
**Sequencing:** strictly sequential (T1 → T7), same git repo, never parallel. Each worker commits alone; never amend landed commits (T1–T17 through `8b6bca6`).

**Verification conventions:** CI-plan text and these todos use plain standard commands (`npm run …`, `npx eslint …`, `cd frontend && npx vue-tsc --noEmit`) — CI runs on a normal filesystem. For IN-SESSION verification on the 9p mount, use the node-path forms in CONTRIBUTING's "9p Checkouts" section (`npm ci --no-bin-links`, `node node_modules/vite/bin/vite.js build`, `node node_modules/prettier/bin/prettier.cjs`, `node node_modules/vitest/vitest.mjs`, and for the new binaries `node node_modules/vue-tsc/bin/vue-tsc.js`, `node node_modules/eslint/bin/eslint.js`). New dev deps install with `--no-bin-links`; verify each bin path after install.

---

## T1 — Root TS + ESLint tooling foundation

**Status:** done

**What changes:** the root npm workspace gains the TypeScript and ESLint toolchain: `tsconfig.json` (new), `eslint.config.js` (new), root `package.json` devDeps + scripts (`typecheck`; `check` and `lint` absorb the new checks). Frontend sources are untouched here — the gates are green trivially at this commit (only `App.vue` is in scope).

**Files:** `package.json`, `tsconfig.json` (new), `eslint.config.js` (new)

**Install (root, 9p-safe form in parentheses):**

```sh
npm i -D typescript@~5.9.3 vue-tsc@^3.3 eslint@^10 eslint-plugin-vue@^10 vue-eslint-parser @vue/eslint-config-typescript@^14 eslint-config-prettier@^10
# (npm ci --no-bin-links after the lockfile update; on 9p the .bin dir stays empty)
```

Pin **typescript ~5.9.3** — TS 7.0.2 is npm `latest` and unsupported by typescript-eslint 8.x (peer `>=4.8.4 <6.1.0`). Never `typescript@7.x`.

**`tsconfig.json` sketch (root):**

```jsonc
{
  "compilerOptions": {
    "target": "ES2022",
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "moduleResolution": "bundler",
    "strict": true,
    "noEmit": true,
    "allowJs": true, // resolves the specs' ../scripts/*.mjs imports (checkJs stays off)
    "skipLibCheck": true,
    "verbatimModuleSyntax": true
  },
  "include": ["frontend/src/**/*.ts", "frontend/src/**/*.vue", "tests/**/*.ts"]
}
```

The ts-only include patterns keep transient `.js` sources out of the program; `allowJs` exists solely so `tests/versioning.spec.ts`'s literal `../scripts/compute-version.mjs` import resolves.

**`eslint.config.js` sketch (root):**

```js
import pluginVue from 'eslint-plugin-vue';
import { defineConfigWithVueTs, vueTsConfigs } from '@vue/eslint-config-typescript';
import eslintConfigPrettier from 'eslint-config-prettier/flat';

export default defineConfigWithVueTs(
  { name: 'app/files-to-lint', files: ['frontend/src/**/*.{ts,vue}', 'tests/**/*.ts'] },
  pluginVue.configs['flat/recommended'],
  vueTsConfigs.recommended,
  eslintConfigPrettier
);
```

Verified by probe: `defineConfigWithVueTs` wires vue-eslint-parser as the main parser for `.vue`, the TS parser as the script-block delegate (needed — `flat/recommended` alone leaves `parserOptions.parser` null and espree chokes on TS script syntax), and scopes TS rules to `**/*.vue` as well. eslint-config-prettier must come last so eslint does not fight the prettier gate.

**Root `package.json` script changes:**

```json
{
  "typecheck": "vue-tsc --noEmit",
  "check": "npm run frontend:build && npm run typecheck && npm run backend:check",
  "lint": "prettier --check . && eslint \"frontend/src/**/*.{ts,vue}\" \"tests/**/*.ts\""
}
```

The eslint invocation uses quoted glob patterns (eslint does the globbing — no shell-expansion/platform ambiguity, no warnings for out-of-scope files).

**Verification:**

1. After install, verify bin paths: `ls node_modules/vue-tsc/bin/vue-tsc.js node_modules/eslint/bin/eslint.js node_modules/typescript/bin/tsc` (9p: `.bin` is empty — node-path invocations are mandatory).
2. `npm run typecheck` — trivially green (only `App.vue` in the program at this point).
3. `npm run lint` — prettier passes (untouched files) and eslint lints `App.vue` only; if a vue-recommended rule trips on the current script, fix the style in place (do not disable the rule).
4. `npm run check` — green; CI workflow yml untouched (the aggregate gates absorb the new checks).
5. Prettier gate on the new files: they must be prettier-formatted (2-space, single quotes, printWidth 100).

**Anti-patterns:** do NOT put the configs inside `frontend/` (root owns the gates — see plan decision 7); do NOT pin TS 7.x; do NOT skip vue-eslint-parser (a peer — install explicitly); do NOT enable `checkJs` (the transient `.js` sources would flood the typecheck); do NOT add `parserOptions.project` (non-type-aware linting per plan decision 4).

**ISC:** ISC-4, ISC-5, ISC-8, ISC-11 (partially — bin paths verified here).

**Status notes (2026-09-05 run):**

- ISC-4, ISC-5, ISC-8 verified green in-session; ISC-11 partial — bin paths verified (`node_modules/vue-tsc/bin/vue-tsc.js`, `node_modules/eslint/bin/eslint.js`, `node_modules/typescript/bin/tsc`), the CONTRIBUTING entry is T6 item 5.
- Deviation 1 — `eslint.config.js` gains the official `configureVueProject({ scriptLangs: ['ts', 'js'] })` call. `defineConfigWithVueTs` injects `vue/block-lang` with `allowNoLang: false` (via its `@vue/typescript/setup` block), so the sketch-as-is fails the lint gate on App.vue's plain `<script>` block. The todo's step-3 contingency ("fix the style in place") cannot apply here: the fix (`lang="ts"`) cascades into strict typing this commit cannot satisfy (TS7006 on the untyped function params, TS2339 on `entries = ref([])` → `Ref<never[]>`), and the `DirectoryEntry` type does not exist until T2/T3 — typing it now would be a hand-written contract surface (ISC-A-1). `scriptLangs: ['ts', 'js']` is the library's documented option for plain script blocks; it sets `allowNoLang: true`, the rule stays active and still rejects non-TS `lang` attributes. App.vue stays untouched and T4 keeps its full scope. Tighten `scriptLangs` to `['ts']` at T4 once the script block is `lang="ts"`.
- Deviation 2 — the lint script runs eslint with `--no-error-on-unmatched-pattern`: eslint 10 exits 2 on a CLI glob that matches no files, and `tests/**/*.ts` matches nothing until T5 migrates the specs. Keep the flag permanently — it is inert once the glob matches (T5 onward) and CI runs the same script.
- In-session verification used node-path forms (9p `.bin` stays empty): `node node_modules/vue-tsc/bin/vue-tsc.js --noEmit`, `node node_modules/prettier/bin/prettier.cjs --check .`, `node node_modules/eslint/bin/eslint.js --no-error-on-unmatched-pattern "frontend/src/**/*.{ts,vue}" "tests/**/*.ts"`, frontend build via `cd frontend && node node_modules/vite/bin/vite.js build && node ../scripts/copy-public.mjs`, backend via `cd backend && export CARGO_HOME=$HOME/.cargo RUSTUP_TOOLCHAIN=stable && cargo check`. The `npm run` forms work in CI.
- Installed: typescript 5.9.3, vue-tsc 3.3.11, eslint 10.10.0, eslint-plugin-vue 10.10.0, vue-eslint-parser 10.4.1, @vue/eslint-config-typescript 14.9.0, eslint-config-prettier 10.1.8.

---

## T2 — Generator emits TS with zod validation

**Status:** done

**What changes:** `scripts/generate-openapi-client.mjs` gains zod-schema emission from `api/openapi.yaml` `components.schemas` and emits TypeScript into `frontend/src/lib/api/generated/client.ts` (renamed from `.js`); `client:generate`'s prettier argument and `contract:check`'s `git diff` paths change to `client.ts`; `frontend/package.json` gains `zod`; the integration spec's literal import specifier and `frontend/README.md`'s generated-client path update in the same commit.

**Files:** `scripts/generate-openapi-client.mjs`, `frontend/src/lib/api/generated/client.ts` (renamed, regenerated), `frontend/package.json`, `tests/generated-client.integration.spec.js`, `frontend/README.md`, root `package.json` (contract scripts)

**Install:** `cd frontend && npm i zod@^4.5.4` — zod is a frontend **runtime dep** (ships in the bundle). 9p: `npm i --no-bin-links` / `npm ci --no-bin-links --prefix frontend` after the lockfile update.

**Target emitted shape for `frontend/src/lib/api/generated/client.ts`** (what the generator template must produce — header of schemas + types, then the typed fetch functions):

```ts
// AUTO-GENERATED FILE. DO NOT EDIT.
// Source: api/openapi.yaml

import { z } from 'zod';

export const DEFAULT_BASE_URL = '';

export const EntryKindSchema = z.enum(['directory', 'file']);
export type EntryKind = z.infer<typeof EntryKindSchema>;

export const DirectoryEntrySchema = z.object({
  kind: EntryKindSchema,
  modifiedAt: z.string().nullable().optional(),
  name: z.string(),
  sizeBytes: z.number().int().min(0).nullable().optional()
});
export type DirectoryEntry = z.infer<typeof DirectoryEntrySchema>;

export const DirectoryListingSchema = z.object({
  currentPath: z.string(),
  entries: z.array(DirectoryEntrySchema),
  parentPath: z.string().nullable().optional()
});
export type DirectoryListing = z.infer<typeof DirectoryListingSchema>;

export const HealthResponseSchema = z.object({ service: z.string(), status: z.string() });
export type HealthResponse = z.infer<typeof HealthResponseSchema>;

export const PublicErrorResponseSchema = z.object({ message: z.string() });
export type PublicErrorResponse = z.infer<typeof PublicErrorResponseSchema>;

export async function getApiV1Directory(
  baseUrl: string = DEFAULT_BASE_URL,
  query: Record<string, string | number | boolean | null | undefined> = {},
  init: RequestInit = {}
): Promise<DirectoryListing> {
  // ... unchanged URLSearchParams/fetch construction from the current template ...
  if (!response.ok) {
    let message = `Request failed: GET /api/v1/directory -> ${response.status}`;
    try {
      const body = PublicErrorResponseSchema.parse(await response.json());
      if (body.message) {
        message = body.message;
      }
    } catch {
      // Non-JSON or non-conforming body: keep the fallback message.
    }
    throw new Error(message);
  }
  return DirectoryListingSchema.parse(await response.json());
}
```

**Generator changes in `scripts/generate-openapi-client.mjs`:**

- Add a schema renderer walking `spec.components.schemas`, mapping OpenAPI nodes to zod: `type: object` + `required`/`properties` → `z.object({...})` (required props plain, optional props `.optional()`); `type: [T, 'null']` → `.nullable().optional()`; `type: string` + `enum` → `z.enum([...])`; `$ref` → the named schema; `array` of `$ref` → `z.array(Schema)`; `format: int64` + `minimum: 0` → `.int().min(0)`.
- Emit all schemas into a header section before the operations; keep the `AUTO-GENERATED FILE. DO NOT EDIT.` header, the `Source: api/openapi.yaml` comment, and `DEFAULT_BASE_URL = ''`.
- Per operation, resolve the 200 response's `$ref` to its schema name and emit the typed return (`Promise<DirectoryListing>`) plus the parse call. Key the response handling off the 200 response's **content presence** (JSON content → schema parse; no content → `Promise<Blob>` via `response.blob()`), replacing the current `rawPath.includes('/download')` hack — same behavior for the current API, more correct for future endpoints.
- `outputPath` → `path.join(root, 'frontend/src/lib/api/generated/client.ts')`.
- Keep the duplicate-name check (`Duplicate generated function name '<name>' (…)`).

**Contract script changes (root `package.json`):**

```json
{
  "client:generate": "node scripts/generate-openapi-client.mjs && npx prettier --write frontend/src/lib/api/generated/client.ts",
  "contract:check": "npm run contract:generate && git diff --exit-code -- api/openapi.yaml frontend/src/lib/api/generated/client.ts"
}
```

**Integration spec specifier** (`tests/generated-client.integration.spec.js:13`): `import { getApiV1Directory, getApiV1Health } from '../frontend/src/lib/api/generated/client.js';` → `'../frontend/src/lib/api/generated/client.ts'` (a `.js` spec importing a `.ts` module resolves through vitest's vite pipeline with the literal `.ts` path).

**`frontend/README.md`:** `src/lib/api/generated/client.js` → `client.ts`; add one sentence that the client is TypeScript with zod-validated responses.

**Verification:**

1. `npm run contract:generate` **twice** — the second run must be byte-identical (idempotency), and prettier must run WITH the repo config (the config is discovered by walk-up from the file; cwd does not matter, but a default-config prettier emits double quotes and a spurious diff — verified by probe).
2. `npm run contract:check` — green (`git diff --exit-code` on both paths).
3. `npm run frontend:build` — green (vite transpiles the `.ts` output without a tsconfig; zod resolves from `frontend/node_modules`).
4. `npm run test:integration` — the integration spec imports the new `client.ts` path and passes against the real backend (the zod parse runs against actual responses).
5. `npm run lint` — the generated file must be prettier-stable; the template emits eslint-clean code so the generated file lints like any other.

**Anti-patterns:** do NOT hand-write the schemas into the generated file (it is AUTO-GENERATED DO NOT EDIT — the generator owns them); do NOT run prettier with default config (spurious diff — use the repo config, which walk-up discovers); do NOT forget the AUTO-GENERATED header or the duplicate-name check; do NOT emit `.nullable()` without `.optional()` for the non-required nullable fields; do NOT leave the `includes('/download')` path hack (key off response content); do NOT forget the integration-spec specifier or `frontend/README.md` in this commit.

**ISC:** ISC-2, ISC-3, ISC-6, ISC-A-1, ISC-A-2.

**Status notes (2026-09-05 run):**

- ISC-2, ISC-3, ISC-A-1, ISC-A-2 verified green in-session; ISC-6 verified with the node-path equivalent (generator + repo-config prettier, then `git diff --exit-code -- api/openapi.yaml frontend/src/lib/api/generated/client.ts` — green; idempotent generate+format, byte-identical second cycle). The `openapi:generate` side is unchanged; the npm-run contract forms work in CI.
- Emitted file matches the todo sketch exactly, including the header, `DEFAULT_BASE_URL = ''`, the schema shapes, and the typed fetch functions. Schemas are topologically sorted by `$ref` so value references resolve at runtime — for the current contract the order is EntryKind, DirectoryEntry, DirectoryListing, HealthResponse, PublicErrorResponse (exactly the sketch order).
- Generator emission notes (same behavior for the current contract, small generalizations for future endpoints): `.int()` applies to int32/int64 formats and any numeric `minimum` renders `.min(n)` (the current contract has only `format: int64` + `minimum: 0` → exactly `.int().min(0)`); JSON content detection keys off the media type's `json` suffix (covers `application/json` and `application/…+json`); JSON content without a `$ref` throws a descriptive generator error instead of silently emitting a wrong shape; path-param `params` are typed `Record<string, string | number | boolean>` (no null/undefined — `encodeURIComponent` rejects those); the `z` import is emitted only when the contract declares schemas (an unused import would trip the lint gate).
- In-session verification used node-path forms (9p `.bin` stays empty): `node scripts/generate-openapi-client.mjs`, `node node_modules/prettier/bin/prettier.cjs --write frontend/src/lib/api/generated/client.ts` (repo config discovered by walk-up), contract-check equivalent `git diff --exit-code -- api/openapi.yaml frontend/src/lib/api/generated/client.ts`, `node node_modules/vue-tsc/bin/vue-tsc.js --noEmit`, frontend build via `cd frontend && node node_modules/vite/bin/vite.js build && node ../scripts/copy-public.mjs`, integration spec `node node_modules/vitest/vitest.mjs --config vitest.config.js --run tests/generated-client.integration.spec.js` (with `export CARGO_HOME=$HOME/.cargo RUSTUP_TOOLCHAIN=stable` for the spawned backend), `node node_modules/prettier/bin/prettier.cjs --check .` + `node node_modules/eslint/bin/eslint.js --no-error-on-unmatched-pattern "frontend/src/**/*.{ts,vue}" "tests/**/*.ts"`. The `npm run` forms work in CI.
- Integration spec green against the real backend: the zod parse ran against actual responses (directory listing entries and the 404 error path both validated at the parse).
- Installed: zod 4.5.4 in frontend (runtime dep, ships in the bundle).

---

## T3 — Frontend lib sources → .ts

**Status:** done

**What changes:** the hand-written frontend sources become TypeScript and every importer's specifier updates in the same commit: `frontend/src/lib/format.js` → `format.ts` (typed), `frontend/src/lib/api/client.js` → `api/client.ts` (typed), `frontend/src/main.js` → `main.ts`; `App.vue`'s extensioned `'./lib/format.js'` import → extensionless `'./lib/format'` (both resolve under `moduleResolution: "bundler"` — extensionless is the convention the wrapper already uses); `tests/frontend-helpers.spec.js`'s format specifier → `.ts`.

**Files:** `frontend/src/lib/format.ts`, `frontend/src/lib/api/client.ts`, `frontend/src/main.ts`, `frontend/src/App.vue` (specifier line only), `tests/frontend-helpers.spec.js` (specifier line only)

**Typed `format.ts` sketch** (bodies unchanged — only signatures gain types):

```ts
const dateFormatter = new Intl.DateTimeFormat(undefined, {
  year: 'numeric',
  month: '2-digit',
  day: '2-digit',
  hour: '2-digit',
  minute: '2-digit'
});

export function formatSize(sizeBytes: number | null | undefined): string {
  // body unchanged; undefined reaches the NaN path -> 'NaN KB' (asserted in tests)
}

export function formatModified(isoString: string | null | undefined): string {
  // body unchanged
}
```

The `number | null | undefined` parameter is what keeps `formatSize(undefined)` → `'NaN KB'` (asserted in `tests/frontend-helpers.spec.js`) a valid call — the API type is `number | null` but the test exercises the undefined path deliberately. Keep the null-handling JSDoc comment above `formatModified`.

**Typed wrapper `api/client.ts` sketch** (structure unchanged — types only):

```ts
import { getApiV1Directory } from './generated/client';
import type { DirectoryListing } from './generated/client';

const DEFAULT_BASE_URL = '';

export function toChildPath(currentPath: string, childName: string): string {
  return currentPath ? `${currentPath}/${childName}` : childName;
}

export function buildDownloadHref(
  currentPath: string,
  fileName: string,
  baseUrl: string = DEFAULT_BASE_URL
): string {
  const params = new URLSearchParams({ p: toChildPath(currentPath, fileName) });
  return `${baseUrl}/api/v1/download?${params.toString()}`;
}

export async function fetchDirectory(
  relativePath: string = '',
  baseUrl: string = DEFAULT_BASE_URL
): Promise<DirectoryListing> {
  return getApiV1Directory(baseUrl, relativePath ? { p: relativePath } : {}, {
    headers: {
      Accept: 'application/json'
    }
  });
}

export type { DirectoryEntry, DirectoryListing } from './generated/client';
```

Parameter types are `string` (not the `''` literal) — callers pass real URLs (the integration spec passes the OS-assigned port URL). The type-only re-export at the bottom is what App.vue imports in T4.

**`main.ts`** — same 4 lines, rename only (`import { createApp } from 'vue'; import App from './App.vue'; createApp(App).mount('#app');`).

**Verification:**

1. `npm run frontend:build` — green (vite resolves the renamed modules; the wrapper's extensionless `'./generated/client'` import still resolves).
2. `npm run typecheck` — the new `.ts` files typecheck under strict; `App.vue` (still a JS script at this point) stays in the program and passes.
3. `npm run lint` — eslint lints the renamed `.ts` files + `App.vue`.
4. `npm run test:integration` — green (the integration spec imports the generated client, untouched here).
5. `git status` — clean, no stray `.js` leftovers in `frontend/src/lib/`.

**Anti-patterns:** do NOT use `any` to silence the checker where a real type exists (ISC-A-4); do NOT restructure the wrapper (types only); do NOT use the `''` literal type for `baseUrl` params (callers pass real URLs); do NOT forget `App.vue`'s and the spec's import specifiers in the same commit (they reference the renamed file).

**ISC:** ISC-1 (partially — lib sources), ISC-4, ISC-9.

**Status notes (2026-09-05 run):**

- ISC-4 and ISC-9 verified green in-session (node-path forms below); ISC-1 partial — `frontend/src` now contains zero hand-written `.js` sources (`git grep -n "\.js" frontend/src` shows only `Schema.parse` matches in the generated `client.ts`); the `lang="ts"` script block is T4.
- ISC-9 verified with evidence: `formatSize(undefined)` → `'NaN KB'` passes against the renamed `format.ts` (tests/frontend-helpers.spec.js runs green, 14/14 across both spec files), and the parameter is typed `number \| null \| undefined` exactly as the sketch specified.
- Deviation 1 — `formatSize`'s body gains one line: `const bytes = sizeBytes ?? NaN;` right after the null guard, with the arithmetic below using `bytes` instead of `sizeBytes`. The sketch's "body unchanged" cannot hold under strict: with the signature `number \| null \| undefined`, the null guard does not narrow away `undefined`, so the unchanged comparison (`sizeBytes < 1024`) and division (`sizeBytes / 1024`) both fail with TS18048 (`'sizeBytes' is possibly 'undefined'` — verified by probe). The `undefined` arm must stay in the signature (the test asserts `formatSize(undefined)` → `'NaN KB'` is a valid call), so the coalescing is the minimal type-safe fix: `undefined` → `NaN` reaches the NaN path exactly as before — probe-verified runtime-identical across every test case (0, 999, 1023, unit boundaries, precision boundaries, -5, undefined, null). No casts, no `any` (ISC-A-4 intact).
- Deviation 2 — `frontend/index.html`'s entry specifier updates in the same commit: `/src/main.js` → `/src/main.ts`. The todo's Files list missed that index.html imports the renamed `main.ts`, and vite resolved the stale `.js` specifier silently through its extension fallback (the build was green with the stale reference). T6's docs-drift list has no index.html item and T7's `git grep -c "\.js" frontend/src` counts only `frontend/src`, so nothing downstream would catch it — fixed here to keep "every importer's specifier updates in the same commit" literal. One-line change; the build/typecheck gates re-verified after it.
- In-session verification used node-path forms (9p `.bin` stays empty): frontend build `cd frontend && node node_modules/vite/bin/vite.js build && node ../scripts/copy-public.mjs`, `node node_modules/vue-tsc/bin/vue-tsc.js --noEmit`, `node node_modules/prettier/bin/prettier.cjs --check .` + `node node_modules/eslint/bin/eslint.js --no-error-on-unmatched-pattern "frontend/src/**/*.{ts,vue}" "tests/**/*.ts"`, integration spec `node node_modules/vitest/vitest.mjs --run tests/generated-client.integration.spec.js` (with `export CARGO_HOME=$HOME/.cargo RUSTUP_TOOLCHAIN=stable` for the spawned backend), other root specs `node node_modules/vitest/vitest.mjs --run tests/frontend-helpers.spec.js tests/versioning.spec.js`. The `npm run` forms work in CI.

---

## T4 — App.vue script block `lang="ts"`, typed in place

**Status:** done

**What changes:** `frontend/src/App.vue`'s `<script setup>` gains `lang="ts"` and in-place typing. The template is untouched; the component stays one piece (ISC-A-3).

**Files:** `frontend/src/App.vue`

**Reference:** the current script block is `App.vue:1–73` — imports (`computed, onMounted, ref` from `'vue'`; `buildDownloadHref, fetchDirectory, toChildPath` from `'./lib/api/client'` extensionless; `formatSize, formatModified` from `'./lib/format.js'` → now extensionless `'./lib/format'`), state refs, `breadcrumbs` computed, `readPathFromUrl`, `updateUrl`, `loadDirectory`, `openDirectory`, the `onMounted` hook + `popstate` listener. No `defineProps`/`defineEmits` — the component takes no props.

**Typing sketch** (existing logic, typed — not rewritten):

```vue
<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { buildDownloadHref, fetchDirectory, toChildPath } from './lib/api/client';
import type { DirectoryEntry } from './lib/api/client';
import { formatSize, formatModified } from './lib/format';

const entries = ref<DirectoryEntry[]>([]);

async function loadDirectory(path: string, updateHistory = true): Promise<void> {
  isLoading.value = true;
  try {
    const listing = await fetchDirectory(path);
    currentPath.value = listing.currentPath;
    entries.value = listing.entries;
    errorMessage.value = '';
  } catch (error) {
    errorMessage.value =
      error instanceof Error ? error.message : 'The shared directory is unavailable.';
  } finally {
    isLoading.value = false;
  }
}
// ... readPathFromUrl(): string, updateUrl(path: string): void,
//     openDirectory(path: string): void, breadcrumbs computed — typed in place ...
</script>
```

Typing work: ref generics (`entries = ref<DirectoryEntry[]>([])`), function signatures, the `unknown` catch narrowing (the existing `error instanceof Error` pattern works unchanged under strict), and the mixed import-extension conventions unified (extensionless for both — the wrapper convention). The `popstate` listener param type is inferred from the DOM lib.

**Verification:**

1. `npm run frontend:build` — green: **verify immediately** that vite 8 (rolldown, transpile-only) transpiles the `lang="ts"` script without a tsconfig (premortem risk — if this breaks, stop and adjust before anything downstream commits).
2. `npm run typecheck` — vue-tsc now performs the full SFC check (the `.vue` script block is TypeScript); the strict refs/signatures/narrowing must typecheck with no `any`-casts (ISC-A-4).
3. `npm run lint` — eslint lints the `.vue` script block through the vue-eslint-parser + TS-delegate wiring.
4. `npm run test:integration` — green.

**Anti-patterns:** do NOT restructure App.vue into multiple components (typed in place); do NOT `any`-cast to silence the checker; do NOT remove the popstate listener or add an AbortController (scope creep — parked in the prior run's A22 notes); do NOT touch the template.

**ISC:** ISC-1 (complete — zero `.js` in `frontend/src`), ISC-4 (full SFC check now meaningful), ISC-A-3, ISC-A-4.

**Status notes (2026-09-05 run):**

- ISC-1 (complete), ISC-4, ISC-A-3, ISC-A-4 verified green in-session (node-path forms below). `frontend/src` contains zero `.js` sources and the script block is `lang="ts"`, typed in place; `git diff` shows script-block hunks only — the template and style are byte-unchanged.
- Deviation 1 — `eslint.config.js` changes in this commit: `configureVueProject({ scriptLangs: ['ts', 'js'] })` → `scriptLangs: ['ts']` (T1 deviation 1's flagged note, applied on schedule). The option was added at T1 because `defineConfigWithVueTs` injects `vue/block-lang` with `allowNoLang` tied to the `scriptLangs` contents (source: `allowNoLang: scriptLangs.includes("js")` in its `@vue/typescript/setup` block) and the plain script block could not be typed yet; now the script block IS `lang="ts"`, so the allowlist tightens. The rule stays active and rejects non-TS lang attributes — probed both dimensions with temporary `.vue` files (removed after each run): a plain `<script setup>` fails with `The 'lang' attribute of '<script>' is missing` and `lang="js"` fails with `Only 'ts' can be used for the 'lang' attribute of '<script>'`, while App.vue's `lang="ts"` passes (correct — the T1 loose-config contingency is retired).
- Premortem check green immediately: vite 8 (rolldown, transpile-only) transpiles the `lang="ts"` script without a tsconfig — the frontend build passed right after the edit, before anything downstream committed.
- Typing in place with no any-casts (ISC-A-4): `entries = ref<DirectoryEntry[]>([])` (the ref generic is required — `ref([])` infers `Ref<never[]>` under strict), typed signatures (`loadDirectory(path: string, updateHistory = true): Promise<void>`, `readPathFromUrl(): string`, `updateUrl(path: string): void`, `openDirectory(path: string): void`), the existing `error instanceof Error ? … : …` catch narrowing works unchanged under strict (`unknown`), and the popstate listener param type is inferred from the DOM lib. `currentPath`/`errorMessage`/`isLoading` stay unannotated — their initial values infer `Ref<string>`/`Ref<boolean>`. Logic NOT rewritten, no defineProps/defineEmits (the component takes no props), template untouched (ISC-A-3).
- In-session verification used node-path forms (9p `.bin` stays empty): frontend build `cd frontend && node node_modules/vite/bin/vite.js build && node ../scripts/copy-public.mjs`, `node node_modules/vue-tsc/bin/vue-tsc.js --noEmit`, `node node_modules/prettier/bin/prettier.cjs --check .` + `node node_modules/eslint/bin/eslint.js --no-error-on-unmatched-pattern "frontend/src/**/*.{ts,vue}" "tests/**/*.ts"`, integration spec `node node_modules/vitest/vitest.mjs --run tests/generated-client.integration.spec.js` (with `export CARGO_HOME=$HOME/.cargo RUSTUP_TOOLCHAIN=stable` for the spawned backend), other root specs `node node_modules/vitest/vitest.mjs --run tests/frontend-helpers.spec.js tests/versioning.spec.js`. The `npm run` forms work in CI.
- All gates green: vite build (111 modules, 895ms), vue-tsc full SFC check, prettier, eslint, integration test 2/2 against the real backend, other root specs 14/14.

---

## T5 — Root tests migrate to .ts

**Status:** done

**What changes:** the three root specs migrate to TypeScript: `tests/versioning.spec.js` → `.ts`, `tests/frontend-helpers.spec.js` → `.ts`, `tests/generated-client.integration.spec.js` → `.ts`. Import specifiers finalized. `vitest.config.js` needs no change (its include glob already covers `tests/**/*.{test,spec}.{js,ts}`).

**Files:** the three spec files (renamed), no config change

**Spec header sketch:**

```ts
import { describe, expect, it } from 'vitest';
import { formatModified, formatSize } from '../frontend/src/lib/format';

// ... versioning.spec.ts keeps its literal '../scripts/compute-version.mjs' import —
// the root tsconfig's allowJs resolves it (checkJs stays false, so the .mjs types stay loose)
```

Import specifiers: frontend sources may be extensionless (`.ts` resolved under bundler) or extensioned `.ts` — both work under the root tsconfig and vitest's vite pipeline; pick extensionless to match the wrapper's convention. The integration spec keeps its `node:*` imports (`node:child_process`, `node:fs`, `node:os`, `node:path`, `node:timers/promises`) and its literal `client.ts` path from T2; `@types/node` at root covers them.

**Verification:**

1. `npm run test:integration` — all three `.ts` specs pass (vitest transpiles TS through its own vite pipeline, no extra dep).
2. `npm run typecheck` — vue-tsc now checks the `.ts` specs too (ISC-4 covers `tests/`); the `.mjs` script import resolves via `allowJs`.
3. `npm run lint` — eslint lints the `.ts` specs (`tests/**/*.ts` in scope).
4. `git status` — clean; zero `.spec.js` files remain at root.

**Anti-patterns:** do NOT move tests into `frontend/` (root vitest owns `tests/**` — plan gotcha 9); do NOT change `vitest.config.js`'s include glob or `requireAssertions: true`; do NOT change the `formatSize(undefined)` assertion (the typing accommodates it — ISC-9); do NOT migrate `scripts/*.mjs` (out of scope).

**ISC:** ISC-7, ISC-4 (tests now in the typecheck), ISC-9.

**Status notes (2026-09-05 run):**

- ISC-7 and ISC-4 verified green in-session (node-path forms below); ISC-9 untouched — the `formatSize(undefined)` → `'NaN KB'` assertion is byte-unchanged and stays valid (the `number \| null \| undefined` param from T3 accommodates it).
- Deviation 1 — the integration spec's generated-client import specifier changes in this commit: the literal `'../frontend/src/lib/api/generated/client.ts'` path set at T2 → extensionless `'../frontend/src/lib/api/generated/client'`. The todo's "frontend sources may be extensionless or extensioned `.ts` — both work under the root tsconfig" cannot hold for the extensioned form: TS5097 (`An import path can only end with a '.ts' extension when 'allowImportingTsExtensions' is enabled`) fires once the spec is `.ts` and in the root tsconfig's include — at T2 the spec was still `.js`, outside the include, so vue-tsc never saw the specifier and the error never surfaced. Adding `allowImportingTsExtensions` would violate the todo's "no config change" (Files: "the three spec files (renamed), no config change"), so the todo's own preference resolves it: "pick extensionless to match the wrapper's convention" — the specifier now matches `api/client.ts`'s `'./generated/client'` import exactly. Vite's resolver default extensions include `.ts`, so the extensionless import resolves in the vitest run (verified below). No downstream check depends on the spec's specifier (T6's docs items and T7's checks reference the generated file and docs, not the spec import).
- Deviation 2 — the specs gain typed-in-place annotations beyond the rename (the strict tsconfig from T1 forces param annotations — TS7006). `versioning.spec.ts`: `createFakeGit`'s destructured options gain an inline object type (`{ lastVersionCommit?: string; height?: string; branch?: string }`) and its git mock args are `string[]` (probed: `vi.fn` calls then type, so `git.mock.calls.map(([args]) => args[0])` narrows cleanly); `let tempRoot: string` (probed: reads of a declared-but-uninitialized `let` inside vitest callbacks do NOT trigger TS2454 — control-flow analysis is function-local). `generated-client.integration.spec.ts`: the helpers gain signatures (`stripAnsiEscapes(text: string): string`, `waitForListenPort(startedAt: number): Promise<number>`, `waitForBackendReady(url: string, startedAt: number): Promise<void>`, `waitForExit(child: ChildProcess, timeoutMs: number): Promise<boolean>`, `signalBackendGroup(signal: NodeJS.Signals): void`), and the spawn handle is typed `BackendProcess = ChildProcessByStdio<null, Readable, Readable> \| undefined` — the exact shape `stdio: ['ignore', 'pipe', 'pipe']` produces (stdin ignored, stdout/stderr piped, non-null streams). Probe-verified dimension: an explicit `ChildProcess` annotation (const or let) degrades the spawn overload resolution to plain `ChildProcess` (stdout `Readable \| null`, TS18047 on `.on('data', …)`), while the matching ByStdio declared type keeps the precise overload and the `.stdout`/`.stderr` accesses typecheck with zero body changes. `signalBackendGroup` gains one guard: `if (backendProcess?.pid)` inside the existing try — behavior-identical (the catch already handled the gone group; with the guard the kill is skipped instead of throwing-and-catching), and no non-null assertion (ISC-A-4's spirit; the afterAll already guards `backendProcess?.pid` the same way).
- In-session verification used node-path forms (9p `.bin` stays empty; npm-run forms work in CI): all specs `node node_modules/vitest/vitest.mjs --run` (with `export CARGO_HOME=$HOME/.cargo RUSTUP_TOOLCHAIN=stable` for the spawned backend), `node node_modules/vue-tsc/bin/vue-tsc.js --noEmit`, `node node_modules/prettier/bin/prettier.cjs --check .`, `node node_modules/eslint/bin/eslint.js --no-error-on-unmatched-pattern "frontend/src/**/*.{ts,vue}" "tests/**/*.ts"`. `vitest.config.js` untouched (include glob already covers `.ts`, `requireAssertions: true` stays). `@types/node` resolved without an install (22.19.19 transitive at root per the plan).
- All gates green: vitest 16/16 across the three `.ts` specs (14 unit + 2 integration against the real backend — the zod parse ran against actual responses, 404 error path included), vue-tsc (the `.ts` specs are now in the typecheck program; the `.mjs` script import resolves via `allowJs`), prettier, eslint (`tests/**/*.ts` in scope — the flag is now inert). Renames via `git mv`; zero `.spec.js` files remain at root.

---

## T6 — Docs drift sweep

**Status:** done

**What changes:** every doc that mentions frontend tooling and goes stale from this migration, in the same change. The research page's [Docs drift](../research/frontend-typescript-migration.md#docs-drift-this-migration-creates) list has exact line references; items 1–3 are the silent ones (false statements / unapplied instructions).

**Files** (from the research page's docs-drift list):

1. `docs/features/rust-vue-migration-contract-first.md:76` — the T17 correction "no frontend typecheck — the frontend is JavaScript-only; TypeScript migration is parked as backlog" becomes false → reword to state the typecheck exists (vue-tsc) and the frontend is TypeScript. Leave the Alternatives list (line 27) historical unless amending the ADR.
2. `.github/instructions/backend-frontend-architecture.instructions.md:2` — `applyTo: 'backend/src/**/*.rs,frontend/src/**/*.{js,vue},frontend/vite.config.js,package.json'` — the `{js,vue}` glob stops matching migrated `.ts` files → `{js,ts,vue}`.
3. `.github/instructions/testing-quality.instructions.md:2` — same `{js,vue}` drift → `{js,ts,vue}` (its tests glob already includes `ts`).
4. `README.md` Scripts table — `npm run lint # Prettier validation` → reword for prettier + eslint; add a `typecheck` entry (and note `check` now includes it).
5. `CONTRIBUTING.md` — Engineering Standards gate list (`check`, `lint`, `test`, `build`) and the 9p Checkouts list (4 verified forms) — add the 5th form for the new binaries:
   ```sh
   5. node node_modules/vue-tsc/bin/vue-tsc.js --noEmit        (instead of npm run typecheck)
      node node_modules/eslint/bin/eslint.js "frontend/src/**/*.{ts,vue}" "tests/**/*.ts"
   ```
6. `.github/copilot-instructions.md:27–28` — the generic "type checks" language and the quality-gates list — now actually true; update the gates list if wording names specific scripts.
7. `.github/agents/feature-delivery.agent.md:24` — same generic gates list.
8. `.github/pull_request_template.md:20` — `npm run lint` checkbox — still true (lint grew eslint); reword if it names prettier specifically.
9. `docs/features/file-download.md:84`, `docs/features/secure-directory-listing.md:101`, `docs/features/public-release-license-and-footer.md:41` — Validation Evidence lists — generic; touch only if the aggregate script definitions changed their meaning.
10. `docs/architecture/0002-rust-backend-vue-frontend-contract-first.md` — the "stable, typed API interactions" language becomes true; Alternatives section is historical — update only if amending the ADR.
11. `frontend/README.md` — already updated at T2 (generated-client path); verify no other stale claims remain.

**Verification:**

1. `grep -rn "JavaScript-only\|no frontend typecheck" docs/ .github/` — zero hits after the sweep.
2. `grep -n "applyTo" .github/instructions/*.md` — both architecture and testing-quality globs match `.ts`.
3. `npm run lint` — prettier gate on all touched docs.
4. `npm run contract:check` — still green (docs don't affect the contract chain).

**Anti-patterns:** do NOT mark acceptance criteria `[x]` falsely (the T13 lesson — only claim what exists); do NOT touch historical ADR Alternatives wording unless amending the ADR; do NOT claim a typecheck exists before T1–T5 land (this sweep runs AFTER the code todos); do NOT add Rust files to prettier scope.

**ISC:** ISC-10.

**Status notes (2026-09-05 run):**

- Items 1–5 changed: the T17 correction reworded (`docs/features/rust-vue-migration-contract-first.md` now states the typecheck exists — vue-tsc --noEmit over frontend/src and tests/ — and the frontend is TypeScript; Alternatives list left historical); both instruction `applyTo` globs `{js,vue}` → `{js,ts,vue}` (only the frontend/src glob changed — testing-quality's tests glob already covered `.ts`); README Scripts table reworded (`lint` → "Prettier + ESLint", new `typecheck` entry, `check` description notes the added typecheck); CONTRIBUTING's 9p Checkouts list gained the 5th form for the new binaries.
- Deviation 1 — the 9p Checkouts list: the sketch's bare eslint line gained the note "for the eslint check in `npm run lint`", and item 3's replacement note tightened to "the prettier check in `npm run lint`" (lint grew eslint, so the prettier form no longer replaces `npm run lint` wholesale — the two 9p forms each cover one half of the gate). The eslint invocation matches the todo's authoritative form exactly (no `--no-error-on-unmatched-pattern`); verified in-session that the form without the inert flag exits 0 (the glob matches files since T5, so the flag is a no-op).
- Items 6–9 verified no-change-needed: `.github/copilot-instructions.md`, `.github/agents/feature-delivery.agent.md`, `.github/pull_request_template.md`, and the three feature Validation Evidence lists all name still-valid script names (`npm run check` / `lint` / `test` / `build`) with no prettier-specific or false wording — their meanings grew (typecheck in `check`, eslint in `lint`) but nothing became false, and the README table is where the concrete toolchain descriptions live. The Engineering Standards gate list in CONTRIBUTING keeps its 4 script names — `npm run check` includes the typecheck, so no separate entry; the todo's concrete change for item 5 was the 9p form only.
- Items 10–11 verified no-change-needed: ADR 0002's "stable, typed API interactions" language is now true (nothing false remains; Alternatives left historical — the ADR is not amended), and `frontend/README.md` has zero `client.js` references and no JS-only claims (the T2 update covers it).
- In-session verification (9p, node-path forms): `grep -rn "JavaScript-only\|no frontend typecheck" docs/ .github/` zero hits; `grep -n applyTo .github/instructions/*.md` both globs match `.ts`; `grep -rn "client\.js" README.md CONTRIBUTING.md docs/ .github/ frontend/ --include="*.md"` zero hits; `node node_modules/prettier/bin/prettier.cjs --check .` green; `node node_modules/eslint/bin/eslint.js "frontend/src/**/*.{ts,vue}" "tests/**/*.ts"` green (exit 0); contract-check equivalent `node scripts/generate-openapi-client.mjs && node node_modules/prettier/bin/prettier.cjs --write frontend/src/lib/api/generated/client.ts && git diff --exit-code -- api/openapi.yaml frontend/src/lib/api/generated/client.ts` green (no drift); `node node_modules/vue-tsc/bin/vue-tsc.js --noEmit` green (regression check). The `npm run` forms work in CI.

---

## T7 — Final verification + plan status flip

**What changes:** end-to-end gate verification against the full ISC checklist, then flip this plan page's status header Draft → done and update `wiki/index.md`'s status column.

**Files:** `wiki/plans/frontend-typescript.md` (status header), `wiki/index.md` (status column)

**Verification (the full checklist, plain commands as CI runs them):**

1. `npm run check` — `frontend:build && typecheck && backend:check` all green (ISC-4, ISC-8).
2. `npm run lint` — prettier + eslint green (ISC-5).
3. `npm run test` — frontend build + backend tests + integration spec green (ISC-7).
4. `npm run contract:generate && npm run contract:check` — idempotent, both diff paths green (ISC-6).
5. `git grep -c "\.js" frontend/src` — zero `.js` sources in `frontend/src` (ISC-1); `git grep -n "lang=\"ts\"" frontend/src/App.vue` — the script block is TS.
6. `git grep -n "Schema.parse" frontend/src/lib/api/generated/client.ts` — each JSON-returning generated function validates its response (ISC-2, ISC-3).
7. `grep -rn "AUTO-GENERATED" frontend/src/lib/api/generated/` — the header survived (ISC-A-2).
8. `.github/workflows/ci.yml` — **unchanged** (zero workflow changes; the aggregate gates absorbed the new checks — ISC-8).
9. Review the ISC checkboxes in the plan page: mark each verified item, fix any gap before flipping the status.

**Anti-patterns:** do NOT flip the status to done with a gap in the ISC checklist; do NOT amend landed commits; do NOT run parallel workers.

**ISC:** all — this todo is the final gate.

**Status:** done

**Status notes (2026-09-05 run):**

- Full gate verification green in-session, then the plan page's status flipped to Done and `wiki/index.md`'s plan-row status column updated (Active → Done; the research page's row stays Active — it is the scout page, not this plan).
- Gate evidence (node-path forms, 9p `.bin` stays empty; the npm-run forms work in CI): frontend build `cd frontend && node node_modules/vite/bin/vite.js build` + `node ../scripts/copy-public.mjs`, `node node_modules/vue-tsc/bin/vue-tsc.js --noEmit`, backend `cargo check` — all green (ISC-4, ISC-8); `node node_modules/prettier/bin/prettier.cjs --check .` + `node node_modules/eslint/bin/eslint.js --no-error-on-unmatched-pattern "frontend/src/**/*.{ts,vue}" "tests/**/*.ts"` both exit 0 (ISC-5); `cargo test` 31/31 and root `node node_modules/vitest/vitest.mjs --run` 16/16 (14 unit + 2 integration against the real backend — the zod parse ran against actual responses, 404 error path included) (ISC-7).
- Contract chain (ISC-6): generate + repo-config prettier TWICE — second cycle byte-identical (idempotent, sha1sum-compared) — then `git diff --exit-code -- api/openapi.yaml frontend/src/lib/api/generated/client.ts` green.
- ISC-1 verified with the `-n` form: `git grep -n "\.js" frontend/src` shows exactly 5 matches, all `.json()` method calls in the generated client (the todo's `-c` form would count those lines — the zero-`.js`-sources claim verified via `find frontend/src -name "*.js"` → 0 plus the -n inspection); `App.vue:1` is `<script setup lang="ts">`. ISC-2/ISC-3 verified: 3 generated fetch functions — `getApiV1Directory`/`getApiV1Health` validate their JSON responses via `Schema.parse` (return-path lines 71/152, error-path lines 61/103/142), `getApiV1Download` returns `Promise<Blob>` via `response.blob()` with its error path parsed (no schema, no parse for the binary success path per plan). ISC-A-2: the AUTO-GENERATED header survived at `client.ts:1`.
- ISC-9/ISC-10/ISC-11/ISC-A-1/ISC-A-3/ISC-A-4 re-verified in-session: `formatSize(undefined)` → `'NaN KB'` assertion byte-unchanged (`tests/frontend-helpers.spec.ts:40`) with the `number \| null \| undefined` param; zero "JavaScript-only"/"no frontend typecheck" hits and both `applyTo` globs match `.ts`; CONTRIBUTING:26–28 documents the 5th node-path form (bin paths exercised successfully in-session); zero `.d.ts` under `frontend/src`; App.vue typed in place (T4 evidence); zero any-casts in `frontend/src` and `tests/`.
- ISC-8's CI leg: `git log --oneline c560656..HEAD -- .github/workflows/ci.yml` empty (zero workflow changes); ci.yml runs the aggregate scripts (`npm run check`, `npm run lint`) so the new checks ride the existing steps.
- Plan page updated in this commit: all 15 ISC checkboxes `[x]`, status header `**Status:** Done`, a one-line **Completed:** note under the header block; `wiki/index.md`'s plan row Active → Done.
- Deviation 1 (wording note only) — the todo's step-5 command `git grep -c "\.js" frontend/src` reads as "count = zero", but the count form counts the generated file's `.json()` method-call lines (5). The intended check is the -n inspection (matches are only `.json()` method calls) plus `find` for actual `.js` sources — verified that way. No behavioral gap. Same wording drift: T7's header text says "Draft → done" while the todo list's page header was already "Active" — flipped Active → Done per the plan's living-doc convention.
- The `npm run` forms work in CI; in-session verification used the node-path forms throughout (9p `.bin` stays empty).
