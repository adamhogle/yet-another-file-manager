# Rust Test Code Coverage

## Summary

The Rust backend measures test code coverage with `cargo-llvm-cov` and CI enforces a minimum line-coverage threshold on every push and pull request.

## Problem Statement

Rust tests existed without any coverage measurement: test completeness was tracked qualitatively through audit-driven coverage maps, and untested paths could silently accumulate. Without a measured number and a CI gate, coverage regressions are invisible.

## User Story

As a maintainer, I want the CI quality job to measure Rust test coverage and fail below the agreed threshold, so that coverage gaps surface during review instead of in production.

## Scope

- In scope: coverage measurement for the backend library code and the in-repo integration tests (`backend/tests/`), via `cargo-llvm-cov` with LLVM source-based line coverage.
- In scope: a `backend:coverage` npm script and a `quality`-job CI step that gates on the threshold.
- In scope: a checked-in coverage baseline of the accepted uncovered lines, enforced by a `backend:coverage:baseline` npm script so that a new untested branch fails CI however small it is.

## Non-Goals

- Not in scope: frontend (Vitest) coverage measurement.
- Not in scope: coverage of the two binary targets (`backend/src/main.rs` startup wiring and `backend/src/bin/openapi.rs` spec generator); the gate measures the library, where all behavior lives. The CLI argument parsing in `main.rs` is tested through its own unit tests but its runtime serving loop is not exercised by the test suite.
- Not in scope: line-level coverage exclusions inside source files. The stable Rust toolchain has no `#[coverage(off)]` attribute (nightly-only), and the accepted gaps are recorded in the baseline instead of hidden by source annotations.

## UX / Flow

Not applicable (CI tooling).

## Technical Notes

- Tool: `cargo-llvm-cov` (LLVM source-based coverage, the current Rust standard). The invocation is `cargo llvm-cov --lib --tests --ignore-filename-regex 'src/(main\.rs|bin/)' --fail-under-lines <threshold>`: `--lib` plus `--tests` covers the library target and the integration test targets and excludes the two bin targets.
- CI installs `cargo-llvm-cov` through `taiki-e/install-action` (fast, pinned to the lockfile version locally via `cargo install --locked`).
- The threshold is 99 percent library line coverage, matching the AGENTS.md rule that every critical line of code must have coverage. The measured number on `main` is 99.02 percent with 44 accepted uncovered lines, recorded per file in `backend/coverage-baseline.json`. The remaining 44 missed lines are: two tool artifacts (a `tracing::warn!` macro branch and a rust-embed derive branch attributed to macro source), two I/O race windows (a file vanishing between the download's metadata read and open, and a listing entry vanishing between canonicalize and metadata), a defensive panic guard, defensive type-conversion failures the config validation already rules out upstream, and llvm-cov's cross-binary instantiation records, which count gap-region lines of branches that individual test binaries never call even though other binaries cover them.
- Branch and region coverage are reported but not gated: LLVM region/branch data carries compiler-generated noise for async functions that would make a gate flaky.
- A percentage floor alone hides small regressions: five new uncovered lines in the 4300-line lib move the total by about 0.1 percent, still above the floor. The baseline closes that hole: `backend/coverage-baseline.json` records the exact number of accepted uncovered lines per file (44 total on `main`), and `npm run backend:coverage:baseline` (script `scripts/check-coverage-baseline.mjs`) compares the current report against it and fails when any file gains uncovered lines. Raising the baseline requires reviewing the new uncovered lines; the report JSON (`target/coverage.json`, emitted by the same coverage run) is the source the comparison reads. The floor stays as a coarse guard for large shifts and tool-chain changes.

## Security Considerations

No runtime impact. The gate exists to protect security-relevant paths (path traversal, symlink escape, access control) from losing test coverage; the coverage report makes every handler branch visible during review.

## Acceptance Criteria

- [ ] `npm run backend:coverage` runs the Rust test suite under coverage and fails below the threshold.
- [ ] `npm run backend:coverage:baseline` fails when any file gains uncovered lines.
- [ ] The CI `quality` job includes the coverage gate, installs the tool, and checks the baseline.
- [ ] The measured library line coverage meets the threshold on `main`.

## Test Plan

- Unit: the gate is exercised by every existing and new Rust test.
- Integration: CI runs the gate on push and pull request.
- End-to-end/manual: `cargo llvm-cov --lib --tests` locally before pushing.

## Open Questions

- None.
