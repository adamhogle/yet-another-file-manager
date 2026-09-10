# Copilot Stack

This repository installs a stricter Copilot customization stack aligned to secure Rust backend and Vue frontend file-manager development.

## Installed Set

1. Repository baseline instructions: `.github/copilot-instructions.md`
2. Targeted instructions:
   - `.github/instructions/backend-frontend-architecture.instructions.md`
   - `.github/instructions/file-security.instructions.md`
   - `.github/instructions/testing-quality.instructions.md`
3. Specialized agents:
   - `.github/agents/feature-delivery.agent.md`
   - `.github/agents/product-requirements.agent.md`
   - `.github/agents/architecture-review.agent.md`
   - `.github/agents/test-strategy.agent.md`
   - `.github/agents/security-audit.agent.md`
4. Domain skills:
   - `.github/skills/file-manager-domain/SKILL.md`
   - `.github/skills/feature-delivery-governance/SKILL.md`
5. Documentation templates:
   - `docs/templates/FEATURE_SPEC.md`
   - `docs/templates/ADR.md`

## Why This Set

- Keeps feature implementation aligned with the Rust API + Vue frontend split.
- Enforces secure handling of user-controlled file paths.
- Requires user stories, feature specs, and architecture records for non-trivial work.
- Improves quality by requiring tests, deterministic validation, and acceptance-criteria mapping.
- Establishes reusable workflows for product definition, architecture review, implementation, test strategy, and security review.

## Expected Workflow

1. Use the product requirements agent to shape the user story.
2. Write a feature spec from the template in `docs/features/` for every behavior-changing feature.
3. Use the architecture review agent when the design changes boundaries or abstractions.
4. Use the feature delivery agent to implement the change.
5. Use the test strategy and security audit agents before calling the work complete.

## Repository Rule

Every feature that changes product behavior starts with a spec file in `docs/features/`.

Implementation can refine the spec, but it should not bypass it.
