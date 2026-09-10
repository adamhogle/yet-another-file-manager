# Contributing

Thanks for contributing to Yet Another File Manager.

## Development Setup

1. Use the dev container in `.devcontainer/devcontainer.json`.
2. Install dependencies: `npm install`.
3. Start the app: `npm run dev -- --open`.

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
