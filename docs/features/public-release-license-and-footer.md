# Feature Spec: Public Release License and UI Legal Footer

## User Story

As a repository visitor and end user,
I want clear licensing and source attribution,
so I can understand reuse terms and find the canonical source repository.

## Scope

- Add a top-level `LICENSE` file using AGPL-3.0-only.
- Expose license metadata in Node package manifests.
- Add a compact frontend footer that shows copyright, license, and a link to the GitHub repository.
- Update project README with explicit license and repository reference.

## UX Flow

- On page load, users see a small footer beneath the file listing UI.
- Footer content includes:
  - Current year and project attribution
  - License identifier (`AGPL-3.0-only`)
  - Link to source repository on GitHub

## API and Data Impact

- No backend API changes.
- No contract changes.
- No persistence changes.

## Security Considerations

- Footer repository link opens in a new tab with `rel="noopener noreferrer"` to prevent tabnabbing.
- No user-controlled input is introduced.

## Definition of Done

- `LICENSE` exists at repository root with AGPLv3 text.
- `package.json` and `frontend/package.json` include SPDX license field.
- Footer renders in `frontend/src/App.vue` and includes repository link.
- Root `README.md` includes License section and repository URL.
- Quality checks (`npm run check`, `npm run lint`, `npm run test`) run successfully or any deferral is explicitly documented.
