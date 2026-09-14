---
applyTo: 'backend/**/*.rs,frontend/src/**/*.{js,ts,vue},tests/**/*.{test,spec}.{js,ts},backend/tests/**/*.rs'
---

For changes with behavior impact:

- Add or update tests close to the changed logic.
- Cover expected flow, edge cases, and failure states.
- Avoid brittle snapshot-only assertions.
- Keep tests deterministic and isolated.
- Verify UI changes by rendering them: rebuild the frontend and check every affected state at desktop, tablet and phone widths (for example 1440, 768, 390 and 320 px) against a server that serves real listing data. Breakpoint bugs such as overflowing columns, unreachable actions and panels of inconsistent width do not show up in vitest; capture full-page screenshots per width so before and after states can be compared.
- Ensure local commands pass: `npm run check`, `npm run lint`, `npm run test`.
