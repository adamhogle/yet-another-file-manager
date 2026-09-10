# ADR 0002: Rust Backend + Vue Frontend with Contract-First OpenAPI

## Status

Accepted

## Context

The project currently uses SvelteKit with Node-based server logic for file management. Product direction is to migrate backend execution to Rust and frontend implementation to Vue. This split increases the risk of frontend/backend drift unless API contracts are automatically enforced.

The system handles sensitive file operations where correctness and explicit failure behavior are more important than implementation speed.

## Decision

Adopt a split architecture:

1. Rust service as authoritative backend for file-management operations.
2. Vue application as frontend client.
3. OpenAPI specification generated from Rust handlers/types as the single API contract artifact.
4. Frontend API clients/types generated from OpenAPI; no handwritten request/response models for backend endpoints.
5. CI contract-drift checks that fail when generated artifacts are stale.

Migration will be phased to preserve existing behavior while replacing implementation layers incrementally.

## Alternatives Considered

1. Keep SvelteKit + Node and add stricter TypeScript contracts
2. Rust backend with handwritten frontend API wrappers
3. Vue frontend first, backend migration later without contract tooling

Keeping the existing stack does not meet desired runtime/tooling direction. Handwritten wrappers are high-drift and maintenance-heavy. Frontend-first migration without contract tooling increases breakage risk.

## Consequences

Benefits:

- Stronger backend safety model and predictable performance envelope.
- Clear, versionable API contract artifact shared across teams.
- Reduced frontend/backend divergence via generated clients and CI gates.

Trade-offs:

- Additional generation tooling and CI complexity.
- Temporary dual-stack operation during migration.
- Contributors must learn Rust + OpenAPI generation workflow.

Follow-up work:

- Scaffold backend and frontend workspaces.
- Port file-security invariants into Rust modules and tests.
- Add contract, integration, and migration parity checks.

## Security / Operations Impact

- Trust boundary remains at backend API; frontend is untrusted input producer.
- Rust backend must enforce canonical path validation, traversal prevention, and symlink boundary checks before file operations.
- OpenAPI-driven contract improves explicit error modeling and observability consistency.
- CI enforcement reduces accidental behavior regressions reaching deployment.
- Operational rollout uses phased cutover with parity validation before retiring legacy paths.
