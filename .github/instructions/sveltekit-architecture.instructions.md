---
applyTo: 'backend/src/**/*.rs,frontend/src/**/*.{js,vue},frontend/vite.config.js,package.json'
---

Prefer the Rust backend + Vue frontend split over framework-specific shortcuts.

- Keep Axum handlers focused on request/response orchestration and push reusable logic into backend modules.
- Keep all privileged file-system operations server-side; never leak direct file-system assumptions into frontend code.
- Maintain explicit loading and error states in Vue UI flows.
- Use OpenAPI-generated client contracts in frontend code instead of handwritten endpoint schemas.
- Favor small, testable units for path normalization, root-boundary checks, and error mapping.
