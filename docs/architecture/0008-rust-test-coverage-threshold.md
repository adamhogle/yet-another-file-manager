# Rust Test Coverage Tool and Threshold

## Status

Accepted

## Context

The backend Rust tests had no measured coverage. Test completeness was tracked through audit-driven coverage maps (`.pi/plans/2026-09-04-repo-health/plan.md`, ISC-17), but nothing enforced or quantified it: untested handler branches could accumulate silently and coverage regressions were invisible in review.

## Decision

Measure coverage with `cargo-llvm-cov` (LLVM source-based coverage) and gate the CI `quality` job on a 99 percent library line-coverage floor, invoked as `cargo llvm-cov --lib --tests --ignore-filename-regex 'src/(main\\.rs|bin/)' --fail-under-lines 99 --lcov --output-path target/coverage.lcov`. The measurement covers the library target and the integration test targets and excludes the two bin targets (`main.rs` runtime serving loop, `openapi.rs` spec generator), where the behavior under test is either exercised through the library or is process wiring. The measured number is 99.02 with 44 summary-counted missed lines, of which llvm-cov's own uncovered-lines listing shows 6; the baseline records those 6.

The same coverage run feeds a checked-in baseline: `backend/coverage-baseline.json` records the accepted uncovered line numbers per file, taken from the lcov export whose per-file `DA:` records with a zero count are exactly the lines llvm-cov's `--show-missing-lines` listing shows, and `scripts/check-coverage-baseline.mjs` fails on any uncovered line number the baseline does not declare. A percentage floor alone hides small regressions (five new uncovered lines in the 4300-line lib move the total by about 0.1 percent, still above the floor), and a per-file count comparison hides a new uncovered line that is offset by a newly covered line in the same file; the per-line baseline closes both holes so a new untested branch fails CI however small it is. Raising the baseline is a deliberate review decision; the floor stays as a coarse guard for large shifts and tool-chain changes.

## Alternatives Considered

1. `cargo-tarpaulin`: simpler install, but slower, Linux-only container assumption matches, yet it is less maintained and its line accounting differs from the standard source-based model.
2. Gate on all targets including bins: pulls the measured number down by roughly five points for process wiring that no in-repo test should boot (a bound TCP listener and a shutdown loop), and pushes maintainers toward `#[coverage(off)]` annotations rather than honest tests.
3. No gate, report only: rejected; the number only stays honest when regressions fail CI.
4. `#[coverage(off)]` annotations on the artifact functions: the clean mechanism for genuinely uncoverable lines, but the attribute is nightly-only (E0658 on the stable 1.98 toolchain this repository pins), and applying it to a function excludes its covered logic along with the artifact.
5. Percentage floor only, no baseline: rejected; a floor alone cannot catch small regressions, which is exactly where an untested key branch hides.

## Consequences

Coverage regressions now break CI on push and pull request in two ways: the 99 percent floor keeps the total above the AGENTS.md coverage rule, and the baseline fails on any uncovered line the report lists that the baseline does not accept. A new uncovered line is caught even when an unrelated line in the same file becomes covered at the same time, which a per-file count comparison would net to zero. The threshold is a floor, not a target: the summary counts 44 missed lines, of which llvm-cov's uncovered-lines listing shows 6 (the two I/O race windows, a defensive panic guard, defensive type-conversion failures the config validation already rules out upstream); the tool artifacts (a `tracing::warn!` macro branch and a rust-embed derive branch) and llvm-cov's cross-binary instantiation records, which count gap-region lines of branches that individual test binaries never call even though other binaries cover them, are counted by the floor but not listed per line, so the baseline does not pin them and the floor's resolution is one line. No refactor makes the races deterministic and the artifacts live outside this repository. When an unrelated refactor moves an accepted gap line, the baseline fails on the shifted line number; the fix is a one-line baseline update after review, the same ergonomics as a snapshot test. Artifact-count shifts no longer fail the baseline, which removes the earlier pain where a coverage-neutral change tripped it. Region and branch coverage are reported but not gated because LLVM branch data carries compiler-generated noise for async functions.

## Security / Operations Impact

No runtime or rollout impact. The gate protects the test coverage of security-relevant paths (path traversal, symlink escape, group-based access control, delete authorization); the coverage report keeps every handler branch visible during review.

## Follow-up work

Track branch coverage when rustc stabilizes `-Cbranch-coverage` (cargo-llvm-cov's `--branch` is unstable and the pinned 1.98 toolchain has no branch instrumentation); MC/DC, the strongest practical path metric, is nightly-only (`-Zinstrument-mcdc`). Revisit bin-target coverage if the startup wiring gains a testable seam.
