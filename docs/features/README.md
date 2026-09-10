# Feature Specs

This directory contains feature specifications for behavior-changing work.

## Rule

Every feature that changes product behavior starts with a spec file in this directory before implementation begins.

Implementation can update the spec as details sharpen, but it should not bypass the spec.

## Naming

Use a short, descriptive kebab-case filename.

Examples:

- `secure-directory-listing.md`
- `file-upload-validation.md`
- `rename-and-move-operations.md`

## Required Content

Start from `docs/templates/FEATURE_SPEC.md` and include:

1. Problem statement
2. User story
3. Scope and non-goals
4. UX or API flow
5. Security considerations
6. Acceptance criteria
7. Test plan
