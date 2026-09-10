# Contributing

Thanks for contributing to Yet Another File Manager.

## Development Setup

1. Use the dev container in `.devcontainer/devcontainer.json`.
2. Install dependencies: `npm ci`.
3. Start the app: `npm run dev`.

## 9p Checkouts

Bind mounts served over 9p (Windows Docker Desktop and similar setups) reject
operations native Linux filesystems allow: chmod fails with EPERM, and npm's
bin-link step depends on it. On these checkouts run the commands in their
verified forms:

1. `npm ci --no-bin-links` and `npm ci --no-bin-links --prefix frontend`
   instead of `npm ci` / `npm ci --prefix frontend`
2. `cd frontend && node node_modules/vite/bin/vite.js build`
   instead of `npm run frontend:build`
3. `node node_modules/prettier/bin/prettier.cjs --check .` (or `--write .`)
   instead of the prettier check in `npm run lint` / `npm run format`
4. `node node_modules/vitest/vitest.mjs --config vitest.config.js --run`
   instead of `npm run test:integration`
5. `node node_modules/vue-tsc/bin/vue-tsc.js --noEmit` (instead of `npm run typecheck`)
   and `node node_modules/eslint/bin/eslint.js "frontend/src/**/*.{ts,vue}" "tests/**/*.ts"`
   for the eslint check in `npm run lint`

Line endings need no extra setup: `.gitattributes` fixes them repo-wide for
future clones.

## Supported Runtime

1. Production target is Linux Docker containers only.
2. Windows runtime behavior is out of scope and should not be added accidentally.
3. Path handling should follow Linux/POSIX semantics.

## Engineering Standards

1. Keep changes small and focused.
2. Add or update tests for behavior changes.
3. Run local quality gates before opening a pull request:
   - `npm run check`
   - `npm run lint`
   - `npm run test`
   - `npm run build`
4. Prefer server-side enforcement for file operations and access control.
5. Avoid introducing breaking changes without documenting migration steps.

## Pull Requests

1. Create a descriptive branch name, for example `feat/upload-endpoint` or `fix/path-normalization`.
2. Fill in the pull request template completely.
3. Link related issues and include test evidence.
4. Ensure CI is passing before requesting review.

## Commit Messages

Use clear, imperative commit messages. Example:

- `feat: add directory listing endpoint`
- `fix: prevent path traversal on download route`

## Security

Do not commit secrets or sensitive sample data. Use `.env.example` to document required environment variables.
