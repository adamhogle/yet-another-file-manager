---
name: feature-delivery-governance
description: Guidance for converting feature requests into user stories, specs, architecture records, and test plans
---

# Feature Delivery Governance Skill

Use this skill when a task introduces or changes product behavior.

## Delivery Sequence

1. Clarify the user story.
2. Define scope, non-goals, and acceptance criteria.
3. Write or update a feature spec in `docs/features/` for any feature that changes product behavior.
4. Write or update an architecture record in `docs/architecture/` when design decisions have long-term impact.
5. Define a test strategy before implementation is considered complete.

## User Story Quality Bar

A valid story must identify:

- The actor
- The action or goal
- The value or outcome

Reject stories that only describe implementation tasks.

## Feature Spec Minimum Content

1. Problem statement
2. User story
3. Scope and non-goals
4. UX or API flow
5. Security and operational considerations
6. Acceptance criteria
7. Test plan

## Architecture Record Triggers

Create or update an architecture record when the change:

- Introduces a new service boundary
- Changes data ownership or persistence
- Alters trust boundaries
- Adds new cross-cutting abstractions
- Creates a migration or compatibility concern

## Release Readiness Rule

Do not describe a feature as done if acceptance criteria, docs, and test coverage are incomplete.

Any behavior-changing feature without a corresponding file in `docs/features/` is not ready for implementation or sign-off.
