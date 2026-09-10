---
applyTo: 'backend/**/*.rs,frontend/src/**/*.{js,vue},tests/**/*.{test,spec}.{js,ts},backend/tests/**/*.rs'
---

For changes with behavior impact:

- Add or update tests close to the changed logic.
- Cover expected flow, edge cases, and failure states.
- Avoid brittle snapshot-only assertions.
- Keep tests deterministic and isolated.
- Ensure local commands pass: `npm run check`, `npm run lint`, `npm run test`.
