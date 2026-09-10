# Going Public Implementation Plan

> **Task tracking:** Implement this plan task-by-task, marking each step's checkbox (`- [ ]`) as it completes.

**Goal:** Take the repository from private to a public release with a CalVer version scheme, a tag-driven release pipeline, a hard AGPL-3.0-only license, unsloped public docs, clean history, and working MR/issue setup.

**Architecture:** Keep the existing git-height version calculator, but repurpose it: `version.json` carries `{year, month}` and the computed version becomes `YYYY.MM.N` (N = commit count since the last `version.json` change). Add a tag-triggered release workflow beside the existing PR/main CI, plus gitleaks and dependabot. Rewrite history into 8 milestone commits (7 development milestones plus one public-preparation commit), dropping the internal agent planning directory, then force-push before flipping visibility.

**Tech Stack:** GitHub Actions (CI + release workflows), gitleaks, dependabot, Node 22 + Rust (existing), Markdown.

**Spec:** Decisions confirmed with the maintainer (2026-09-09): license stays AGPL-3.0-only; versioning becomes CalVer `YYYY.MM.N` with git height as N; no code of conduct for now; history squashed to ~8 clean commits; the internal planning directory removed from tracking and history; repo made public only after all gates pass. Existing specs: `docs/features/git-height-versioning.md`, `docs/governance/BRANCH_PROTECTION_CHECKLIST.md`.

## Global Constraints

- License: `AGPL-3.0-only` everywhere. Do not change `LICENSE`, `package.json`, `frontend/package.json`, `backend/Cargo.toml`, `backend/deny.toml`, or the footer.
- Version format: `YYYY.MM.N` with zero-padded month (`2026.09.7`). Tags are `vYYYY.MM.N`.
- No code of conduct file (explicitly deferred by maintainer).
- The internal planning directory must not appear in any commit of the new history, and must be in `.gitignore`.
- Do not push to `origin` until Task 5 is complete; do not flip the repo public until Task 6.
- New and edited Markdown follows the unslop rules from the `unslop` skill: no em dashes, no AI vocabulary, plain prose, sentence case headings for README-level docs.
- Destructive history rewrite: create a local backup tag first and keep it until the maintainer confirms the public repo is healthy.

---

## Task 1: CalVer versioning (YYYY.MM.N)

**Files:**

- Modify: `version.json`
- Modify: `scripts/compute-version.mjs`
- Modify: `tests/versioning.spec.ts`
- Modify: `package.json` (root `version` field)
- Modify: `frontend/package.json` (`version` field)
- Modify: `api/openapi.yaml` (regenerated)
- Modify: `README.md` (Versioning section)
- Modify: `docs/features/git-height-versioning.md`

**Interfaces:**

- Consumes: existing `parseVersionConfig`, `buildVersionInfo`, `resolveVersionInfo`, `formatGitHubOutput` from `scripts/compute-version.mjs`.
- Produces: `parseVersionConfig(raw)` reads `{year, month}`; `buildVersionInfo({year, month, patch, shortSha, branch})` returns `version = "YYYY.MM.N"` (zero-padded month), `releaseLine = "YYYY.MM"`, `releaseVersion = "YYYY.MM.0"`; unchanged `imageVersion`, `shaTag`, `latestTag`, `sha`, `branch`.
- Consumes (later tasks): Task 2's release workflow reads `compute-version.mjs --plain` and `--github-output`; CI workflows are unchanged because output key names stay the same.

- [ ] **Step 0: Commit this plan file so it survives the rewrite**

```bash
git add wiki/plans/2026-09-09-going-public.md
git commit -m "docs: add going-public implementation plan"
```

This must happen before Task 5: the rewrite rebuilds history from `pre-public-backup`, so any uncommitted file is dropped by Task 5 Step 4's `git reset --hard`.

- [ ] **Step 1: Rewrite the version tests first**

Replace the cases in `tests/versioning.spec.ts` with CalVer expectations:

```ts
it('builds calver and main-branch image tags from version inputs', () => {
  const versionInfo = buildVersionInfo({
    year: 2026,
    month: 9,
    patch: 7,
    shortSha: 'abc12345',
    branch: 'main'
  });

  expect(versionInfo).toEqual({
    branch: 'main',
    imageVersion: '2026.09.7',
    latestTag: 'latest',
    year: 2026,
    month: 9,
    patch: 7,
    releaseLine: '2026.09',
    releaseVersion: '2026.09.0',
    sha: 'abc12345',
    shaTag: 'sha-abc12345',
    version: '2026.09.7'
  });
});

it('rejects invalid version config', () => {
  expect(() => parseVersionConfig({ year: 2026, month: 13 })).toThrow(
    'version.json month must be an integer between 1 and 12'
  );
  expect(() => parseVersionConfig({ year: 2026, month: 0 })).toThrow(
    'version.json month must be an integer between 1 and 12'
  );
  expect(() => parseVersionConfig({ year: -1, month: 9 })).toThrow(
    'version.json year must be a non-negative integer'
  );
});
```

Also update the tests that Step 1 does not spell out explicitly, converting every `major`/`minor` usage to `year`/`month`:

- `it('omits latest for non-main branches and formats GitHub output')`: call `buildVersionInfo` with year/month (e.g. `{ year: 2026, month: 4, patch: 2, shortSha: 'deadbeef', branch: 'feature/versioning' }`); the assertions on `latestTag` and `formatGitHubOutput` stay, and add `expect(versionInfo.version).toBe('2026.04.2')` and `expect(versionInfo.releaseLine).toBe('2026.04')`.
- `it('falls back to patch 0 when git log output is empty')`: expect `versionInfo.version` to be `'2026.09.0'` with `{ year: 2026, month: 9 }` in `version.json`.

In the `resolveVersionInfo` describe block, write `version.json` as `JSON.stringify({ year: 2026, month: 9 })`, and update the expectations:

```ts
expect(versionInfo.version).toBe('2026.09.7');
expect(versionInfo.year).toBe(2026);
expect(versionInfo.month).toBe(9);
expect(versionInfo.releaseLine).toBe('2026.09');
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `node node_modules/vitest/vitest.mjs --config vitest.config.js --run tests/versioning.spec.ts`
Expected: FAIL with `buildVersionInfo` producing `0.1.7` instead of `2026.09.7`.

- [ ] **Step 3: Update `version.json`**

```json
{
  "year": 2026,
  "month": 9
}
```

- [ ] **Step 4: Update `scripts/compute-version.mjs`**

Replace `parseVersionConfig`:

```js
export function parseVersionConfig(rawConfig) {
  const year = rawConfig?.year;
  const month = rawConfig?.month;

  if (!Number.isInteger(year) || year < 0) {
    throw new Error('version.json year must be a non-negative integer');
  }

  if (!Number.isInteger(month) || month < 1 || month > 12) {
    throw new Error('version.json month must be an integer between 1 and 12');
  }

  return { year, month };
}
```

Replace `buildVersionInfo`:

```js
export function buildVersionInfo({ year, month, patch, shortSha, branch }) {
  if (!Number.isInteger(patch) || patch < 0) {
    throw new Error('patch must be a non-negative integer');
  }

  if (!shortSha || typeof shortSha !== 'string') {
    throw new Error('shortSha is required');
  }

  const monthPadded = String(month).padStart(2, '0');
  const version = `${year}.${monthPadded}.${patch}`;
  const shaTag = `sha-${shortSha}`;
  const latestTag = branch === 'main' ? 'latest' : '';

  return {
    branch,
    imageVersion: version,
    latestTag,
    year,
    month,
    patch,
    releaseLine: `${year}.${monthPadded}`,
    releaseVersion: `${year}.${monthPadded}.0`,
    sha: shortSha,
    shaTag,
    version
  };
}
```

Update the `resolveVersionInfo` body to destructure `{ year, month }` from `parseVersionConfig` and pass them through:

```js
const { year, month } = parseVersionConfig(rawVersionConfig);
// ...
return buildVersionInfo({
  year,
  month,
  patch,
  shortSha,
  branch: detectedBranch
});
```

`formatGitHubOutput` stays exactly as-is (key names unchanged).

- [ ] **Step 5: Align manifest versions and lockfiles**

Set root `package.json` version and `frontend/package.json` version to `2026.09.0`. Do not change `private: true` or `license` fields.

Update the root fields of the lockfiles by hand (do NOT run `npm install`, it fails in this environment with an arborist bug; do NOT touch dependency versions):

- `package-lock.json`: top-level `"version": "0.1.0"` (line 3) and `packages[""].version` (line 9) to `"2026.09.0"`. Leave `node_modules/yocto-queue`'s `0.1.0` (line ~3097) alone.
- `frontend/package-lock.json`: the two root occurrences (lines 3 and 9) to `"2026.09.0"`.

- [ ] **Step 6: Regenerate the OpenAPI contract**

Run: `npm run openapi:generate`
Expected: `api/openapi.yaml` `info.version` becomes `2026.09.0`.

- [ ] **Step 7: Run the verification suite (contract check after commit)**

Run:

```sh
node node_modules/vitest/vitest.mjs --config vitest.config.js --run tests/versioning.spec.ts
npm run check
npm run lint
npm run test
npm run build
node scripts/compute-version.mjs --plain
```

Expected: versioning spec passes; check/lint/test/build pass; `--plain` prints `2026.09.<height>` (nonzero height is expected before the commit; record it).

Do not run `npm run contract:check` here: it diffs generated files against HEAD, so it fails while the OpenAPI version bump is uncommitted. Run it after the commit in Step 10.

- [ ] **Step 8: Update README Versioning section**

Rewrite the `## Versioning` section to:

```markdown
## Versioning

The repository uses calendar versioning: `YYYY.MM.N`, where `YYYY.MM` is the
release line stored in `version.json` and `N` is the git commit count since the
last change to `version.json`.

- Bump `version.json` when you start a new month's release line (it resets the
  commit counter).
- `npm run version:print` shows the resolved build version for the current
  commit. `--plain` prints just the version, `--release` prints the release
  line version (`YYYY.MM.0`).
- The OpenAPI contract version in `api/openapi.yaml` is derived from the
  release line by `openapi:generate`.

Docker images use `YYYY.MM.N` tags, the `YYYY.MM` release line, a `sha-<sha>`
tag, and `latest` on main. Releases are tagged `vYYYY.MM.N` and published by
the Release workflow.
```

- [ ] **Step 9: Update the versioning feature spec**

Edit `docs/features/git-height-versioning.md`:

- Change the title to `# Feature Spec: CalVer Git-Height Versioning`.
- In Problem Statement and Scope, replace "semver-compatible version" with "CalVer `YYYY.MM.N` version".
- In Acceptance Criteria, add a line: `- [ ] version.json stores {year, month} and the computed version is `YYYY.MM.N` with a zero-padded month.`
- Add an Open Questions entry: whether release tags should later override computed versions on tagged commits remains a follow-up.

- [ ] **Step 10: Commit and verify contract consistency**

```bash
git add version.json scripts/compute-version.mjs tests/versioning.spec.ts package.json package-lock.json frontend/package.json frontend/package-lock.json api/openapi.yaml README.md docs/features/git-height-versioning.md
git commit -m "feat: switch versioning to CalVer YYYY.MM.N with git-height patch"
node scripts/compute-version.mjs --plain
npm run contract:check
```

Expected: `--plain` prints `2026.09.0` (the version.json change is now the last touch of that file); `contract:check` passes clean now that the generated files are committed.

---

## Task 2: Secret scanning, dependabot, and the release workflow

**Files:**

- Create: `.gitleaks.toml`
- Create: `.github/dependabot.yml`
- Create: `.github/workflows/release.yml`
- Modify: `.github/workflows/ci.yml` (add `secret-scan` job)
- Modify: `docs/governance/BRANCH_PROTECTION_CHECKLIST.md` (add `secret-scan` and `docker-image` to required checks)

**Interfaces:**

- Consumes: Task 1's `compute-version.mjs --plain` and `--github-output`.
- Produces: `secret-scan` job in CI (required check name `secret-scan`); `release` workflow triggered by `v*` tags that publishes GHCR tags `YYYY.MM.N`, `YYYY.MM`, `sha-<sha>`, `latest`, and creates a GitHub Release.

- [ ] **Step 1: Create `.gitleaks.toml`**

```toml
# Gitleaks configuration for Yet Another File Manager.
#
# The two RSA private keys in backend/tests/oidc_flow.rs are throwaway test
# keys generated once for the mock identity provider (the file documents
# them). They are not secrets, so scanning them would only produce noise.

[extend]
useDefault = true

[allowlist]
paths = [
  '''backend/tests/oidc_flow.rs'''
]
```

- [ ] **Step 2: Add the secret-scan job to `.github/workflows/ci.yml`**

Insert after the `quality` job (before `docker-image`):

```yaml
secret-scan:
  runs-on: ubuntu-latest
  timeout-minutes: 10

  steps:
    - name: Checkout
      uses: actions/checkout@v4
      with:
        fetch-depth: 0

    - name: Gitleaks
      uses: gitleaks/gitleaks-action@v2
      env:
        GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

- [ ] **Step 3: Create `.github/dependabot.yml`**

```yaml
version: 2
updates:
  - package-ecosystem: npm
    directory: /
    schedule:
      interval: weekly
  - package-ecosystem: npm
    directory: /frontend
    schedule:
      interval: weekly
  - package-ecosystem: cargo
    directory: /backend
    schedule:
      interval: weekly
  - package-ecosystem: github-actions
    directory: /
    schedule:
      interval: weekly
```

- [ ] **Step 4: Create `.github/workflows/release.yml`**

```yaml
name: Release

on:
  push:
    tags:
      - 'v*'

permissions:
  contents: write
  packages: write

concurrency:
  group: release-${{ github.ref }}
  cancel-in-progress: false

jobs:
  publish:
    runs-on: ubuntu-latest
    timeout-minutes: 30

    steps:
      - name: Checkout
        uses: actions/checkout@v4
        with:
          fetch-depth: 0 # full history for git-height versioning

      - name: Setup Node
        uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: npm

      - name: Version must match the tag
        id: version
        run: |
          computed="$(node scripts/compute-version.mjs --plain)"
          echo "computed=$computed"
          if [ "v${computed}" != "${GITHUB_REF_NAME}" ]; then
            echo "::error::tag ${GITHUB_REF_NAME} does not match computed version v${computed}"
            exit 1
          fi
          node scripts/compute-version.mjs --github-output >> "$GITHUB_OUTPUT"

      - name: Set up Docker Buildx
        uses: docker/setup-buildx-action@v3

      - name: Log in to GHCR
        uses: docker/login-action@v3
        with:
          registry: ghcr.io
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}

      - name: Publish runtime image
        uses: docker/build-push-action@v6
        with:
          context: .
          file: ./Dockerfile
          target: runtime
          push: true
          tags: |
            ghcr.io/adamhogle/yet-another-file-manager:${{ steps.version.outputs.version }}
            ghcr.io/adamhogle/yet-another-file-manager:${{ steps.version.outputs.release_line }}
            ghcr.io/adamhogle/yet-another-file-manager:${{ steps.version.outputs.sha_tag }}
            ghcr.io/adamhogle/yet-another-file-manager:latest
          labels: |
            org.opencontainers.image.source=https://github.com/adamhogle/yet-another-file-manager
            org.opencontainers.image.version=${{ steps.version.outputs.version }}
            org.opencontainers.image.revision=${{ github.sha }}
          cache-from: type=gha,scope=runtime
          cache-to: type=gha,mode=max,scope=runtime

      - name: Create GitHub Release
        env:
          GH_TOKEN: ${{ github.token }}
        run: gh release create "$GITHUB_REF_NAME" --generate-notes
```

- [ ] **Step 5: Update the branch protection checklist**

In `docs/governance/BRANCH_PROTECTION_CHECKLIST.md`, change the "Required Status Checks" section to:

```markdown
## Required Status Checks

Configure these required checks to match `.github/workflows/ci.yml`:

- [ ] `quality`
- [ ] `secret-scan`
- [ ] `docker-image`
```

- [ ] **Step 6: Verify the workflows parse**

Run: `node -e "const y=require('yaml');const fs=require('fs');for(const f of ['.github/workflows/ci.yml','.github/workflows/release.yml','.github/dependabot.yml']){y.parse(fs.readFileSync(f,'utf8'));console.log('OK',f)}"`
Expected: all three print OK.

`.gitleaks.toml` is TOML, not YAML, and no TOML parser is installed; validate it by structure instead: `head -20 .gitleaks.toml` and confirm it starts with comments then `[extend]` / `[allowlist]` sections with `useDefault = true` and the `backend/tests/oidc_flow.rs` path.

- [ ] **Step 7: Commit**

```bash
git add .gitleaks.toml .github/dependabot.yml .github/workflows/release.yml .github/workflows/ci.yml docs/governance/BRANCH_PROTECTION_CHECKLIST.md
git commit -m "ci: add secret scanning, dependabot, and tag-driven release workflow"
```

---

## Task 3: Public documentation (SECURITY, CHANGELOG, unsloped README, wiki cleanup)

**Files:**

- Create: `SECURITY.md`
- Create: `CHANGELOG.md`
- Modify: `README.md`
- Modify: `wiki/index.md`
- Modify: `wiki/plans/frontend-typescript.md`, `wiki/research/frontend-typescript-migration.md`
- Modify: `docs/features/oidc-authentication.md`, `docs/architecture/0004-oidc-authentication.md`
- Add: `docs/security-review-2026-09-07.md` (currently untracked; it is a public-friendly transparency document, commit it)

**Interfaces:**

- Consumes: Task 1's version format (`2026.09.0`), AGPL-3.0-only license facts already in the repo.
- Produces: `SECURITY.md` (GitHub links it automatically from the Security tab), `CHANGELOG.md` (Keep a Changelog style).

- [ ] **Step 1: Create `SECURITY.md`**

```markdown
# Security Policy

## Reporting a Vulnerability

Report privately through GitHub: open a security advisory from the
repository's Security tab (Settings, Security, Private vulnerability
reporting). Include the version or commit you tested, how to reproduce it,
and the expected versus actual behavior. Do not open a public issue for
security bugs.

If the advisory form is unavailable, email the maintainer at
adam.h.ogle@gmail.com with "YAFM security" in the subject.

## Supported Versions

Only the latest release line is supported. Security fixes land on `main`
and are rolled into the next tagged release; we do not backport fixes to
older release lines.

## Disclosure

We publish fixes and their release notes in the changelog and in GitHub
release notes. There is no embargo or coordinated disclosure process yet;
expect fixes to ship with the next release once triaged.
```

- [ ] **Step 2: Create `CHANGELOG.md`**

```markdown
# Changelog

All notable changes to this project are documented in this file.

The format is based on Keep a Changelog, and this project uses calendar
versioning: `YYYY.MM.N`, where `N` is the commit count since the release
line was last set in `version.json`. See README, Versioning.

## [Unreleased]

## [2026.09.0] - 2026-09-09

### Added

- Public release under AGPL-3.0-only.
- Calendar versioning (`2026.09.0`) replacing the previous 0.1.x scheme.
- CI secret scanning with gitleaks and dependabot update groups.
- Tag-driven release workflow: `vYYYY.MM.N` tags publish a GHCR image and a
  GitHub release.
- Security policy.

### Changed

- Repository opened to the public; history rewritten to milestone commits.
- README rewritten for readers landing from the public repo.
```

- [ ] **Step 3: Unslop the README**

Apply the unslop checklist to `README.md`:

1. Remove every em dash from `README.md` (13 found by `grep -c "—" README.md`; locate with `grep -n "—" README.md`, do not rely on the line list below because Task 1 changed the file: ~36, 50, 67, 70, 78, 87, 112, 150, 151, 159, 172, 204, 226). Replace each with a comma or a period; restructure the sentence if the dash was parenthetical.
2. Replace the `## Status` section heading and its "Current baseline includes:" list with a short `## Features` section in plain prose, for example:

```markdown
## Features

- Rust backend with the Vue frontend embedded in the binary.
- Directory listing and streaming downloads with Range and ETag support.
- Path traversal and symlink escape protection, verified by tests.
- OIDC authentication (authentik tested) with signed session cookies applied
  to every route, including static assets.
- OpenAPI contract generated from Rust types, with a generated TypeScript
  client and a CI drift check.
- Build, test, lint, and Docker image publishing in CI.
```

3. Convert headings to sentence case: `## Runtime requirements`, `## Runtime configuration`, `## Copilot customization`.
4. Add badges under the title:

```markdown
[![CI](https://github.com/adamhogle/yet-another-file-manager/actions/workflows/ci.yml/badge.svg)](https://github.com/adamhogle/yet-another-file-manager/actions/workflows/ci.yml)
[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0--only-blue)](https://www.gnu.org/licenses/agpl-3.0.txt)
```

5. Add the logo from the frontend assets at the top: `![Yet Another File Manager](frontend/public/yafm-logo.svg)` if it renders as a repo-relative image; otherwise skip it.
6. In the `## License` section, add the copyright line: `Copyright 2026 Adam H. Ogle and contributors.`.
7. In `## Copilot customization` and the Status list, remove AI-ish phrasing such as "Initial governance" and "for consistent AI-assisted development"; state plainly what exists.
8. Remove any remaining AI vocabulary: "seamless", "robust" (as puffery), "ensure" (prefer concrete verbs), "leverage", "delve", "landscape".

- [ ] **Step 4: Verify the README sweep**

Run: `grep -c "—" README.md; grep -niE "\b(leverage|seamless|delve|robust|ensure|landscape|game-changer|cutting-edge)\b" README.md`
Expected: em dash count is 0; the vocabulary grep returns no hits.

- [ ] **Step 5: Unslop the heavy docs**

`docs/features/oidc-authentication.md` (23 em dashes) and `docs/architecture/0004-oidc-authentication.md` (13 em dashes): replace em dashes with commas or periods; leave the technical content untouched. Also check `docs/features/rust-vue-migration-contract-first.md` and `docs/architecture/0003-streaming-download-range-etag.md` (1 each).

`docs/security-review-2026-09-07.md` (26 em dashes) is committed in this task and will be public, so it gets the same em dash pass; on top of that, replace any AI-ish phrases in it (e.g. "landscape", "leverage", "robust" used as puffer) with plain wording, keeping every technical finding and decision intact.

- [ ] **Step 6: Fix wiki references to internal planning paths**

The wiki links into the internal planning directory, which will not exist in the public repo. Do:

1. In `wiki/index.md`, in the `## Prior Runs (legacy artifacts)` list: keep the three summary bullets and their dates, but strip the link markup pointing into the internal planning directory, drop the `Full scout context at ...` sentences, and replace em dashes with periods or commas. Keep the link targets that still exist after Tasks 1-4: `docs/features/oidc-authentication.md`, `docs/architecture/0004-oidc-authentication.md`, `README.md#deployment`.
2. In `wiki/index.md`, rewrite the intro paragraph (lines 3-6): it claims the internal planning directory "stays as-is" and is "untracked", both wrong after Tasks 4-5. Replace the whole paragraph with: `Durable knowledge base for Yet Another File Manager. Each page records verified findings or decisions that outlive a single session.`
3. In `wiki/plans/frontend-typescript.md` and `wiki/research/frontend-typescript-migration.md`, replace every internal planning directory path mention with a plain-text note like `(internal planning directory, not in the public repo)`. Read: `grep -n "internal planning directory" wiki/plans/frontend-typescript.md wiki/research/frontend-typescript-migration.md`.
4. Replace the remaining em dashes in `wiki/index.md` (4 occurrences in the Prior Runs bullets).

- [ ] **Step 7: Commit the security review and docs**

Run a prettier check over the changed Markdown and commit:

```bash
node node_modules/prettier/bin/prettier.cjs --check SECURITY.md CHANGELOG.md README.md wiki/index.md wiki/plans/frontend-typescript.md wiki/research/frontend-typescript-migration.md docs/features/oidc-authentication.md docs/architecture/0004-oidc-authentication.md docs/security-review-2026-09-07.md
git add SECURITY.md CHANGELOG.md README.md wiki docs/features/oidc-authentication.md docs/architecture/0004-oidc-authentication.md docs/security-review-2026-09-07.md
git commit -m "docs: add security policy and changelog, unslop public docs, drop internal plan links"
```

---

## Task 4: Remove the internal planning directory from tracking and ignore it

> Note: the internal planning directory is deliberately not spelled out in this plan. In the commands below `$INTERNAL_DIR` is the repository-root internal agent planning directory that this task removes from tracking; the `.gitignore` snippet uses `<internal-planning-directory>` as the same placeholder.

**Files:**

- Modify: `.gitignore`
- Modify: `.prettierignore`
- Delete from tracking: the internal planning directory (keep files on disk for the maintainer)

**Interfaces:**

- Consumes: nothing.
- Produces: the internal planning directory untracked and ignored; required as a precondition for Task 5's history rewrite.

- [ ] **Step 1: Add to .gitignore**

Append:

```gitignore
# Internal agent planning artifacts (not part of the public repo)
<internal-planning-directory>
```

- [ ] **Step 2: Untrack the internal planning directory**

Run: `git rm -r --cached $INTERNAL_DIR`
Expected: the internal planning directory's files leave the index; files remain on disk.

- [ ] **Step 3: Ignore the SDD workspace in prettier**

Append the agent workspace directory to `.prettierignore` (it already excludes `/static/` and other internal directories). The SDD workspace is git-ignored and absent from CI, but local `prettier --check .` flags it and implementers run prettier locally.

- [ ] **Step 4: Commit**

```bash
git add .gitignore .prettierignore
git commit -m "chore: untrack internal planning artifacts and ignore agent workspaces"
```

- [ ] **Step 5: Verify nothing else internal is tracked**

Run: `git ls-files | grep -E "$INTERNAL_DIR/|node_modules|target/|dev-data/|config/yafm\.config\.yaml" | head`
Expected: no output. Also run `node node_modules/prettier/bin/prettier.cjs --check .` and confirm the only warnings left are untracked workspace files under the agent workspace directory (which should be gone after the .prettierignore update) and frontend/dist (git-ignored).

---

## Task 5: Quality gate, backup, and history rewrite (destructive)

**Files:** history only; no file edits.

**Interfaces:**

- Consumes: Tasks 1-4 commits on top of `d5701f7`.
- Produces: a new `main` with 8 commits (7 milestone trees from the old history plus the final public preparation commit), no internal planning directory anywhere; local tag `pre-public-backup` pointing at the old HEAD.

- [ ] **Step 1: Run the full quality gate**

```sh
node node_modules/prettier/bin/prettier.cjs --check .
npm run check
npm run lint
npm run test
npm run build
```

Verify commands: 9p workaround forms per CONTRIBUTING if running on a 9p mount (this workspace needs them). Expected: all pass.

- [ ] **Step 2: Create the backup tag**

Run: `git tag -a pre-public-backup -m "pre-public-history backup: contains internal planning artifacts and all original commits" HEAD`
Expected: tag exists. This tag is never pushed.

- [ ] **Step 3: Rebuild history as milestone commits**

Run:

```bash
git checkout --orphan public-history
for entry in \
  "caf4ddd:feat: scaffold Rust backend, Vue frontend, and devcontainer" \
  "7d2c382:ci: docker image builds, git-height versioning, and GHCR publishing" \
  "fd1fab4:fix: devcontainer toolchain, path handling, and documentation drift" \
  "42ff65a:feat: streaming downloads, request logging, integration tests, and CI quality gates" \
  "65e13e1:build: TypeScript frontend migration with generated client and zod validation" \
  "c5a10e6:feat: OIDC authentication with signed session cookies" \
  "d5701f7:fix: security hardening (JWKS rotation, timeouts, cookie signing key) and deployment docs" \
  "$(git rev-parse pre-public-backup):chore: public release preparation (CalVer, security policy, CI secret scan, release workflow)"
do
  sha="${entry%%:*}"; msg="${entry#*:}"
  git read-tree "$sha"
  git rm -r --cached --ignore-unmatch $INTERNAL_DIR >/dev/null 2>&1 || true
  git commit -q -m "$msg"
done
```

Why the backup tag for the last entry: `git checkout --orphan` leaves HEAD on an unborn branch, so `git rev-parse HEAD` fails. `pre-public-backup` points at the old main HEAD (Tasks 1-4 on top of the 82 original commits), so the last commit reproduces the final tree minus the internal planning directory.

- [ ] **Step 4: Promote and sync**

```bash
git branch -M main
git reset --hard HEAD
```

- [ ] **Step 5: Verify the rewrite**

```bash
git log --oneline
# Expected: exactly 8 commits, in the order above.
git diff --exit-code pre-public-backup main
echo "exit=$?"
# Expected: exit 0 with NO output. pre-public-backup points at the old main HEAD
# (Tasks 1-4 applied, the internal planning directory already removed from the index by Task 4), and the
# rebuilt main is read-tree'd from that same tree, so the two trees are identical.
git ls-tree -r --name-only main | grep -c "$INTERNAL_DIR" || true
# Expected: 0 (no internal planning directory path in the new main tree)
git log --oneline | grep -c "$INTERNAL_DIR" || true
# Expected: 0 (no commit in the new main history mentions the internal planning directory; the backup tag's
# old commits are excluded because this greps only the current branch)
node scripts/compute-version.mjs --plain
# Expected: 2026.09.0 (version.json changed in the last commit, height 0)
```

- [ ] **Step 6: Tag the first release**

Run:

```bash
git tag -a v2026.09.0 -m "2026.09.0"
```

- [ ] **Step 7: Push the rewritten history**

Run: `git push --force-with-lease origin main && git push origin v2026.09.0`
Expected: push succeeds. The old history is no longer referenced from `origin/main`.

- [ ] **Step 8: Confirm CI is green on the new history**

Watch the Actions run for the pushed main commit. Do not proceed to Task 6 until CI passes.

Note: do not delete `pre-public-backup` yet. It keeps the old commits reachable locally. When the maintainer is satisfied, optionally:

```bash
git tag -d pre-public-backup
git reflog expire --expire=now --all
git gc --prune=now --aggressive
```

---

## Task 6: Manual GitHub repository settings (maintainer, after CI is green)

**Files:** none. Settings are applied in the GitHub web UI. This task is a checklist, not code.

**Interfaces:**

- Consumes: Task 5's pushed main and tag.
- Produces: a public repository.

- [ ] **Step 1: Repo metadata**

Settings, General:

- Description: `Self-hosted web file manager with OIDC auth, Rust backend and Vue frontend.`
- Homepage: leave empty or set to the repo URL.
- Topics: `file-manager`, `rust`, `vue`, `self-hosted`, `docker`, `oidc`, `openapi`.
- Social preview: optional; add a screenshot later.

- [ ] **Step 2: Make the GHCR package public**

Packages, `yet-another-file-manager`: change visibility to public. Without this, external users cannot `docker pull` the published images even though the repo is public.

- [ ] **Step 3: Branch protection on main**

Apply `docs/governance/BRANCH_PROTECTION_CHECKLIST.md`:

- Require pull request before merging.
- Require 1 approving review.
- Dismiss stale approvals.
- Require conversation resolution.
- Require status checks: `quality`, `secret-scan`, `docker-image`.
- Require branches up to date.
- Do not allow force pushes or deletions.
- Allow squash merge only (recommended).

- [ ] **Step 4: Security settings**

Settings, Code security:

- Enable private vulnerability reporting.
- Enable secret scanning.
- Enable push protection.
- Enable dependabot security updates.
- Enable dependabot version updates (the `dependabot.yml` from Task 2 drives them).

- [ ] **Step 5: Flip visibility**

Settings, General: change repository visibility to Public. Confirm the dialog.

- [ ] **Step 6: Post-flip smoke test**

- Unauthenticated `curl -s https://api.github.com/repos/adamhogle/yet-another-file-manager` returns 200 with `"private": false`.
- The `v2026.09.0` GitHub Release exists with generated release notes.
- `docker pull ghcr.io/adamhogle/yet-another-file-manager:2026.09.0` works from a fresh machine (or at least the image is visible).
- Open an issue and a draft PR as a sanity check, then close both.
- WRITE no further commits that reintroduce the internal planning directory or internal paths; the branch protection review flow guards this.

- [ ] **Step 7: Close out**

Update `docs/governance/BRANCH_PROTECTION_CHECKLIST.md` if any applied settings diverged (e.g., review count). Confirm to the maintainer that the backup tag can be deleted locally.

---

## Other missing items considered (and the call taken)

These surfaced during research and were either folded into tasks above or consciously excluded:

| Item                                                                           | Status                                                                                                  |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------- |
| Internal planning docs tracked in git (would leak internal session logs)       | Task 4 + Task 5                                                                                         |
| No SECURITY.md, no CHANGELOG.md                                                | Task 3                                                                                                  |
| No secret scanning in CI (test RSA keys exist in `backend/tests/oidc_flow.rs`) | Task 2 (allowlisted)                                                                                    |
| No dependabot                                                                  | Task 2                                                                                                  |
| No tag-based release path (CI only publishes on main push)                     | Task 2                                                                                                  |
| GHCR package visibility (private by default even with a public repo)           | Task 6                                                                                                  |
| Branch protection not applied (only a checklist doc exists)                    | Task 6                                                                                                  |
| `docs/security-review-2026-09-07.md` untracked, contains useful transparency   | Task 3                                                                                                  |
| Repo description, topics, badges, logo                                         | Task 3 + Task 6                                                                                         |
| Code of conduct                                                                | Excluded by maintainer                                                                                  |
| Issue labels beyond GitHub defaults                                            | Excluded; default labels are enough for launch                                                          |
| Screenshots / demo in README                                                   | Excluded for now; optional follow-up                                                                    |
| NPM publishing (`private: true`)                                               | Excluded; this is a Docker-first project                                                                |
| Docker security scanning (Trivy) of the image                                  | Excluded; cargo-deny + npm audit + gitleaks cover the launch bar, Trivy can be a follow-up              |
| AGPL copyright headers per source file                                         | Excluded; the LICENSE and footer cover it, and adding headers everywhere is churn. Follow-up if desired |
| Signed commits                                                                 | Excluded; note it in the checklist as optional hardening                                                |

## Self-Review

1. Spec coverage: maintainer requirements are mapped: versioning (Task 1), pipeline (Task 2, existing CI), license (no work needed, verified in research; AGPL-3.0-only already in LICENSE, both package.json, Cargo.toml, footer, README), unslop (Task 3), history rewrite (Task 5), MRs/issues setup (already present in .github; Task 2 extends, Task 6 applies settings), other missing items (table above).
2. Placeholder scan: no TBD/TODO. File contents are given inline. Verification commands are given per step.
3. Type consistency: `compute-version.mjs` outputs `year`, `month`, `patch`, `version`, `releaseLine`, `releaseVersion` consistently across Task 1 tests, README, and Task 2's workflow (`version`, `release_line` from `--github-output` remains unchanged). Tag format `vYYYY.MM.N` consistent between the release workflow's assert, Task 5's tag, and Task 3's changelog.
