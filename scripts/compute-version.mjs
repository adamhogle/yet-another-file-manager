import { execFile as execFileCallback } from 'node:child_process';
import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';

const execFile = promisify(execFileCallback);
const VERSION_FILE = 'version.json';

export function parseVersionConfig(rawConfig) {
  const year = rawConfig?.year;
  const month = rawConfig?.month;

  if (!Number.isInteger(year) || year < 0) {
    throw new Error('version.json year must be a non-negative integer');
  }

  if (!Number.isInteger(month) || month < 1 || month > 12) {
    throw new Error('version.json month must be an integer between 1 and 12');
  }

  return { year, month };
}

export function buildVersionInfo({ year, month, patch, shortSha, branch }) {
  if (!Number.isInteger(patch) || patch < 0) {
    throw new Error('patch must be a non-negative integer');
  }

  if (!shortSha || typeof shortSha !== 'string') {
    throw new Error('shortSha is required');
  }

  const monthPadded = String(month).padStart(2, '0');
  const version = `${year}.${monthPadded}.${patch}`;
  const shaTag = `sha-${shortSha}`;
  const latestTag = branch === 'main' ? 'latest' : '';

  return {
    branch,
    imageVersion: version,
    latestTag,
    year,
    month,
    patch,
    releaseLine: `${year}.${monthPadded}`,
    releaseVersion: `${year}.${monthPadded}.0`,
    sha: shortSha,
    shaTag,
    version
  };
}

export function formatGitHubOutput(versionInfo) {
  return [
    `version=${versionInfo.version}`,
    `image_version=${versionInfo.imageVersion}`,
    `release_line=${versionInfo.releaseLine}`,
    `release_version=${versionInfo.releaseVersion}`,
    `sha=${versionInfo.sha}`,
    `sha_tag=${versionInfo.shaTag}`,
    `branch=${versionInfo.branch}`,
    `latest_tag=${versionInfo.latestTag}`
  ].join('\n');
}

async function runGit(args, cwd) {
  const { stdout } = await execFile('git', args, { cwd });
  return stdout.trim();
}

export async function resolveVersionInfo({ root = process.cwd(), git = runGit } = {}) {
  const versionFilePath = path.join(root, VERSION_FILE);
  const rawVersionConfig = JSON.parse(await fs.readFile(versionFilePath, 'utf8'));
  const { year, month } = parseVersionConfig(rawVersionConfig);

  const [lastVersionCommit, shortSha, detectedBranch] = await Promise.all([
    git(['log', '-n', '1', '--format=%H', '--', VERSION_FILE], root),
    git(['rev-parse', '--short=8', 'HEAD'], root),
    process.env.GITHUB_REF_NAME
      ? Promise.resolve(process.env.GITHUB_REF_NAME)
      : git(['rev-parse', '--abbrev-ref', 'HEAD'], root)
  ]);

  const patch = lastVersionCommit
    ? Number.parseInt(await git(['rev-list', '--count', `${lastVersionCommit}..HEAD`], root), 10)
    : 0;

  return buildVersionInfo({
    year,
    month,
    patch,
    shortSha,
    branch: detectedBranch
  });
}

async function main() {
  const versionInfo = await resolveVersionInfo();

  if (process.argv.includes('--github-output')) {
    console.log(formatGitHubOutput(versionInfo));
    return;
  }

  if (process.argv.includes('--plain')) {
    console.log(versionInfo.version);
    return;
  }

  if (process.argv.includes('--release')) {
    console.log(versionInfo.releaseVersion);
    return;
  }

  console.log(JSON.stringify(versionInfo, null, 2));
}

const directRunPath = process.argv[1] ? path.resolve(process.argv[1]) : null;
const modulePath = fileURLToPath(import.meta.url);

if (directRunPath === modulePath) {
  main().catch((error) => {
    console.error(error.message);
    process.exit(1);
  });
}
