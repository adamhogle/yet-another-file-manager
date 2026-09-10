---
applyTo: 'backend/src/**/*.rs'
---

When changing file-system logic, enforce these controls:

- Resolve all paths against an allowed root directory.
- Reject absolute paths and path traversal attempts (`..`).
- Handle symlinks carefully and block root escape.
- Validate file names against platform constraints.
- Return safe error messages that avoid leaking host paths.
- Add tests for both success and hostile inputs.
