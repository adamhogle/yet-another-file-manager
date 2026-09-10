# Yet Another File Manager

A web-based file manager for self-hosted local file sharing, built with a Rust backend and Vue frontend.

## Runtime Requirements

- Linux container runtime only (Docker deployment target)
- Windows host/container runtime is not supported

## Status

Current baseline includes:

- Rust backend API with embedded frontend static assets
- Vue frontend application
- OpenAPI generation from Rust endpoints and generated frontend API client
- Contract drift checks in CI
- Cargo tests plus frontend integration tests via Vitest
- Initial governance and CI workflows
- Copilot repo customizations for consistent AI-assisted development

## Quickstart

```sh
npm install
npm run dev
```

## Runtime Configuration

The backend expects a YAML or JSON runtime config file.

- The `backend` binary accepts an optional first CLI argument for the config file path.
- If the argument is omitted, it falls back to `config.yaml` then `config.json` in the current working directory.
- If neither file exists in the current working directory, startup fails.

Configuration fields:

- `sharedRoot` (required): absolute directory path to expose
- `showHidden` (optional): include hidden entries in listings
- `listenAddress` (optional, default `0.0.0.0`): backend bind address
- `listenPort` (optional, default `8080`): backend bind port

## Scripts

```sh
npm run dev               # Build frontend and run Rust backend
npm run frontend:dev      # Run Vue dev server only
npm run check             # Cargo check + frontend build
npm run contract:check    # Regenerate OpenAPI/client and fail on drift
npm run lint              # Prettier validation
npm run test              # Cargo tests + integration tests
npm run build             # Build release Rust binary with embedded frontend
```

## Governance

- Contribution rules: `CONTRIBUTING.md`
- Branch policy checklist: `docs/governance/BRANCH_PROTECTION_CHECKLIST.md`
- Pull request template: `.github/pull_request_template.md`
- Issue templates: `.github/ISSUE_TEMPLATE/`
- CI workflow: `.github/workflows/ci.yml`

## Copilot Customization

Project-level AI guidance is defined in:

- `.github/copilot-instructions.md`
- `.github/instructions/`
- `.github/agents/`
- `.github/skills/`

See `docs/ai/COPILOT_STACK.md` for rationale and usage guidance.

## License

This project is licensed under the GNU Affero General Public License v3.0 only (AGPL-3.0-only).

- Full license text: `LICENSE`
- Source repository: https://github.com/adamhogle/yet-another-file-manager
