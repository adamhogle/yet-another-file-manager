# Copilot Instructions for Yet Another File Manager

You are helping build a secure, self-hosted file-sharing manager with a Rust backend API and Vue frontend.

## Product Priorities

1. Security and correctness over speed of implementation.
2. Predictable UX for file operations (upload, move, rename, delete).
3. Explicit error states and observability.
4. Small, testable changes.

## Technical Priorities

1. Keep privileged file-system logic on the server.
2. Normalize and validate all user-controlled paths.
3. Prevent path traversal and symlink escape attacks.
4. Prefer explicit typed request/domain contracts in Rust and keep OpenAPI as the API source of truth.
5. Add tests for critical edge cases in file operations.

## Workflow Expectations

1. Before coding, state assumptions and acceptance criteria.
2. Do not treat a feature as implementation-ready until the user story, constraints, and success criteria are explicit.
3. For any feature that changes product behavior, create or update a feature spec in `docs/features/` before implementation starts.
4. For architectural changes, create or update an architecture record in `docs/architecture/`.
5. Include tests and docs updates with behavior changes.
6. Every feature should have a clear validation plan covering type checks, lint, tests, and any feature-specific scenarios.
7. Use `npm run check`, `npm run lint`, `npm run test`, and `npm run build` as quality gates.

## Delivery Guardrails

1. A valid user story should identify the actor, goal, and expected outcome.
2. Reject or rewrite ambiguous user stories before implementation.
3. Feature specs should cover scope, UX flow, API/data impacts, security concerns, and definition of done.
4. Architecture records should explain the decision, alternatives considered, and operational consequences.
5. Tests are required for behavior changes unless the change is documentation-only.
6. If tests are deferred, the deferral must be explicit, justified, and treated as delivery risk.
7. A feature implementation is incomplete if `docs/features/` does not contain the current feature spec.
