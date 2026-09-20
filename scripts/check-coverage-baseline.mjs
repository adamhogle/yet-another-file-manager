// Compares the current coverage report against the checked-in baseline of
// accepted uncovered lines. Fails when any file gains uncovered lines, so a
// new untested branch cannot slip in under a percentage floor however small
// it is. Raising the baseline is a deliberate review decision, documented in
// docs/architecture/0008-rust-test-coverage-threshold.md.
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const root = path.dirname(fileURLToPath(import.meta.url)) + '/..';
const reportPath = path.join(root, 'backend/target/coverage.json');
const baselinePath = path.join(root, 'backend/coverage-baseline.json');

let report;
try {
  report = JSON.parse(readFileSync(reportPath, 'utf8'));
} catch {
  console.error(
    'No coverage report found at backend/target/coverage.json. Run `npm run backend:coverage` first.'
  );
  process.exit(1);
}
const baseline = JSON.parse(readFileSync(baselinePath, 'utf8'));

const current = new Map();
for (const file of report.data[0].files) {
  if (!file.filename.includes('/backend/src/')) continue;
  if (file.filename.includes('/backend/tests/')) continue;
  if (file.filename.includes('/backend/src/bin/')) continue;
  if (file.filename.includes('/backend/src/main.rs')) continue;
  const name = path.basename(file.filename);
  current.set(name, file.summary.lines.count - file.summary.lines.covered);
}

const problems = [];
for (const [name, missed] of current) {
  const allowed = baseline[name];
  if (allowed === undefined) {
    problems.push(`${name}: not in the baseline (current ${missed} uncovered lines)`);
  } else if (missed > allowed) {
    problems.push(`${name}: baseline allows ${allowed}, current ${missed} (+${missed - allowed})`);
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

const total = [...current.values()].reduce((sum, missed) => sum + missed, 0);
console.log(`Coverage baseline matches (${total} accepted uncovered lines).`);
