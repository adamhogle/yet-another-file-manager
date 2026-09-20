// Compares the current coverage report against the checked-in baseline of
// accepted uncovered lines. The report is the lcov export: per-file `DA:`
// records whose execution count is zero are exactly the uncovered lines
// llvm-cov's own `--show-missing-lines` listing shows. Fails on any
// uncovered line number a file does not declare, so a new untested branch
// cannot slip in under a percentage floor however small it is. Raising the
// baseline is a deliberate review decision, documented in
// docs/architecture/0008-rust-test-coverage-threshold.md.
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const root = path.dirname(fileURLToPath(import.meta.url)) + '/..';
const reportPath = path.join(root, 'backend/target/coverage.lcov');
const baselinePath = path.join(root, 'backend/coverage-baseline.json');

let report;
try {
  report = readFileSync(reportPath, 'utf8');
} catch {
  console.error(
    'No coverage report found at backend/target/coverage.lcov. Run `npm run backend:coverage` first.'
  );
  process.exit(1);
}
const baseline = JSON.parse(readFileSync(baselinePath, 'utf8'));

// Collects the uncovered line numbers per file, keyed on the path relative
// to `backend/src/` so two files sharing a name in different directories
// cannot collide. Files outside the gate's scope (the bin targets, the test
// sources, the runtime serving loop) are skipped.
const current = new Map();
let record = null;
for (const line of report.split('\n')) {
  if (line.startsWith('SF:')) {
    const filename = line.slice(3);
    record = null;
    if (
      filename.includes('/backend/src/') &&
      !filename.includes('/backend/tests/') &&
      !filename.includes('/backend/src/bin/') &&
      !filename.includes('/backend/src/main.rs')
    ) {
      const uncovered = [];
      current.set(filename.split('/backend/src/')[1], uncovered);
      record = uncovered;
    }
  } else if (record && line.startsWith('DA:')) {
    const [lineNumber, count] = line.slice(3).split(',');
    if (Number(count) === 0) record.push(Number(lineNumber));
  } else if (line.startsWith('end_of_record')) {
    record = null;
  }
}

const problems = [];
for (const [name, uncovered] of current) {
  const allowed = baseline[name];
  if (allowed === undefined) {
    if (uncovered.length > 0) {
      problems.push(`${name}: not in the baseline (uncovered lines ${uncovered.join(', ')})`);
    } else {
      console.error(
        `warning: ${name} is fully covered and not in the baseline; register it only if it gains uncovered lines`
      );
    }
  } else {
    const newLines = uncovered.filter((line) => !allowed.includes(line));
    if (newLines.length > 0) {
      problems.push(`${name}: uncovered lines not in the baseline: ${newLines.join(', ')}`);
    }
    const covered = allowed.filter((line) => !uncovered.includes(line));
    if (covered.length > 0) {
      console.error(
        `warning: the ${name} baseline lists lines that are now covered (${covered.join(', ')}); prune them`
      );
    }
  }
}
for (const name of Object.keys(baseline)) {
  if (!current.has(name)) {
    problems.push(`${name}: in the baseline but no longer part of the coverage report`);
  }
}

if (problems.length > 0) {
  console.error('Coverage baseline mismatch:');
  for (const problem of problems) console.error(`  ${problem}`);
  console.error(
    'Update backend/coverage-baseline.json only after reviewing the new uncovered lines (see docs/architecture/0008-rust-test-coverage-threshold.md).'
  );
  process.exit(1);
}

const total = Object.values(baseline).reduce((sum, lines) => sum + lines.length, 0);
console.log(`Coverage baseline matches (${total} accepted uncovered lines).`);
