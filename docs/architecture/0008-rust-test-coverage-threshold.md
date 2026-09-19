# Title

Rust Test Coverage Tool and Threshold

## Status

Accepted

## Context

The backend Rust tests had no measured coverage. Test completeness was tracked through audit-driven coverage maps (`.pi/plans/2026-09-04-repo-health/plan.md`, ISC-17), but nothing enforced or quantified it: untested handler branches could accumulate silently and coverage regressions were invisible in review.

## Decision

Measure coverage with `cargo-llvm-cov` (LLVM source-based coverage) and gate the CI `quality` job on a 99 percent library line-coverage floor, invoked as `cargo llvm-cov --lib --tests --ignore-filename-regex 'src/(main\\.rs|bin/)' --fail-under-lines 99 --json --output-path target/llvm-cov/coverage.json --summary-only`. The measurement covers the library target and the integration test targets and excludes the two bin targets (`main.rs` runtime serving loop, `openapi.rs` spec generator), where the behavior under test is either exercised through the library or is process wiring. The measured number is 99.02 with 44 accepted uncovered lines.

The same coverage run feeds a checked-in baseline: `backend/coverage-baseline.json` records the exact number of accepted uncovered lines per file (44 total on `main`), and `scripts/check-coverage-baseline.mjs` compares the report JSON against it and fails when any file gains uncovered lines. A percentage floor alone hides small regressions (five new uncovered lines in the 4300-line lib move the total by about 0.1 percent, still above the floor); the baseline closes that hole so a new untested branch fails CI however small it is. Raising the baseline is a deliberate review decision; the floor stays as a coarse guard for large shifts and tool-chain changes.

## Alternatives Considered

1. `cargo-tarpaulin`: simpler install, but slower, Linux-only container assumption matches, yet it is less maintained and its line accounting differs from the standard source-based model.
2. Gate on all targets including bins: pulls the measured number down by roughly five points for process wiring that no in-repo test should boot (a bound TCP listener and a shutdown loop), and pushes maintainers toward `#[coverage(off)]` annotations rather than honest tests.
3. No gate, report only: rejected; the number only stays honest when regressions fail CI.
4. `#[coverage(off)]` annotations on the artifact functions: the clean mechanism for genuinely uncoverable lines, but the attribute is nightly-only (E0658 on the stable 1.98 toolchain this repository pins), and applying it to a function excludes its covered logic along with the artifact.
5. Percentage floor only, no baseline: rejected; a floor alone cannot catch small regressions, which is exactly where an untested key branch hides.

## Consequences

Coverage regressions now break CI on push and pull request in two ways: the 99 percent floor keeps the total above the AGENTS.md coverage rule, and the baseline fails on any single new uncovered line in any file. The threshold is a floor, not a target: the remaining 44 missed lines are two tool artifacts (a `tracing::warn!` macro branch and a rust-embed derive branch), two I/O race windows (a file vanishing between the download's metadata read and open, and a listing entry vanishing between canonicalize and metadata), a defensive panic guard, defensive type-conversion failures the config validation already rules out upstream, and llvm-cov's cross-binary instantiation records, which count gap-region lines of branches that individual test binaries never call even though other binaries cover them. No refactor makes the races deterministic and the artifacts live outside this repository. Because the artifact counts can shift when inlining changes from unrelated refactors, the baseline occasionally fails on a coverage-neutral change; the fix is a one-line baseline update after review, the same ergonomics as a snapshot test. Region and branch coverage are reported but not gated because LLVM branch data carries compiler-generated noise for async functions.

## Security / Operations Impact

No runtime or rollout impact. The gate protects the test coverage of security-relevant paths (path traversal, symlink escape, group-based access control, delete authorization); the coverage report keeps every handler branch visible during review.

## Follow-up work

Track branch coverage when rustc stabilizes `-Cbranch-coverage` (cargo-llvm-cov's `--branch` is unstable and the pinned 1.98 toolchain has no branch instrumentation); MC/DC, the strongest practical path metric, is nightly-only (`-Zinstrument-mcdc`). Revisit bin-target coverage if the startup wiring gains a testable seam.
