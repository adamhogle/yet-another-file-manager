---
name: file-manager-domain
description: Guidance for implementing secure file-management workflows in a Rust backend with Vue frontend
---

# File Manager Domain Skill

## Scope

Use this skill for features involving:

- Directory listing
- Upload/download
- Rename/move/delete
- Metadata and permission checks

## Required Planning Inputs

Before implementation, ensure the task has:

1. A user story with actor, goal, and outcome
2. Acceptance criteria
3. A feature spec for non-trivial changes
4. An architecture record when boundaries or abstractions change

## Design Rules

1. Server-only file I/O.
2. Explicit root directory boundary.
3. Input validation before side effects.
4. Structured error mapping for user-safe messages.
5. Test hostile path and race-condition scenarios.
6. Keep product language, UX expectations, and system behavior aligned.

## Required Documentation Outputs

1. Feature specs belong in `docs/features/`.
2. Architecture records belong in `docs/architecture/`.
3. Test intent should be explicit in the spec or implementation summary.

## Suggested Building Blocks

- Path normalization and root-boundary validation logic in `backend/src/lib.rs` (or extracted backend modules)
- File operation orchestration in backend service code under `backend/src/`
- Vue UI flow orchestration in `frontend/src/` using generated client functions from `frontend/src/lib/api/generated/`

## Done Criteria

Do not describe a file-management feature as complete unless:

1. The user story still makes sense after implementation details are known.
2. Security boundaries are documented and enforced.
3. Automated tests cover core success and hostile-input paths.
4. Any architectural changes are documented.
