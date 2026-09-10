import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  buildVersionInfo,
  formatGitHubOutput,
  parseVersionConfig,
  resolveVersionInfo
} from '../scripts/compute-version.mjs';

const LAST_VERSION_COMMIT = 'aaaaaaaa11111111bbbbbbbb22222222cccccccc3333';

function createFakeGit({
  lastVersionCommit = LAST_VERSION_COMMIT,
  height = '7',
  branch = 'main'
} = {}) {
  return vi.fn(async (args) => {
    if (args[0] === 'log') return lastVersionCommit;
    if (args[0] === 'rev-list') return height;
    if (args[0] === 'rev-parse') {
      return args.includes('--abbrev-ref') ? branch : 'aaaaaaaa';
    }
    throw new Error(`unexpected git args: ${args.join(' ')}`);
  });
}

describe('git-height versioning', () => {
  it('builds semver and main-branch image tags from version inputs', () => {
    const versionInfo = buildVersionInfo({
      major: 0,
      minor: 1,
      patch: 7,
      shortSha: 'abc12345',
      branch: 'main'
    });

    expect(versionInfo).toEqual({
      branch: 'main',
      imageVersion: '0.1.7',
      latestTag: 'latest',
      major: 0,
      minor: 1,
      patch: 7,
      releaseLine: '0.1',
      releaseVersion: '0.1.0',
      sha: 'abc12345',
      shaTag: 'sha-abc12345',
      version: '0.1.7'
    });
  });

  it('omits latest for non-main branches and formats GitHub output', () => {
    const versionInfo = buildVersionInfo({
      major: 1,
      minor: 4,
      patch: 2,
      shortSha: 'deadbeef',
      branch: 'feature/versioning'
    });

    expect(versionInfo.latestTag).toBe('');
    expect(formatGitHubOutput(versionInfo)).toContain('sha_tag=sha-deadbeef');
    expect(formatGitHubOutput(versionInfo)).toContain('latest_tag=');
  });

  it('rejects invalid version config', () => {
    expect(() => parseVersionConfig({ major: 0, minor: '1' })).toThrow(
      'version.json minor must be a non-negative integer'
    );
  });
});

describe('resolveVersionInfo', () => {
  let tempRoot;

  beforeEach(async () => {
    tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), 'yafm-version-'));
    await fs.writeFile(
      path.join(tempRoot, 'version.json'),
      JSON.stringify({ major: 1, minor: 2 }),
      'utf8'
    );
    vi.stubEnv('GITHUB_REF_NAME', '');
  });

  afterEach(async () => {
    vi.unstubAllEnvs();
    await fs.rm(tempRoot, { recursive: true, force: true });
  });

  it('rejects when version.json is missing', async () => {
    await expect(
      resolveVersionInfo({ root: path.join(tempRoot, 'missing'), git: createFakeGit() })
    ).rejects.toThrow(/ENOENT/);
  });

  it('computes the patch height from git log output', async () => {
    const git = createFakeGit({ height: '7' });
    const versionInfo = await resolveVersionInfo({ root: tempRoot, git });

    expect(versionInfo.version).toBe('1.2.7');
    expect(versionInfo.patch).toBe(7);
    expect(versionInfo.major).toBe(1);
    expect(versionInfo.minor).toBe(2);
    expect(versionInfo.releaseLine).toBe('1.2');
    expect(versionInfo.sha).toBe('aaaaaaaa');
    expect(versionInfo.branch).toBe('main');
    expect(versionInfo.latestTag).toBe('latest');
    expect(git.mock.calls.map(([args]) => args[0])).toContain('rev-list');
  });

  it('falls back to patch 0 when git log output is empty', async () => {
    const git = createFakeGit({ lastVersionCommit: '', height: '7' });
    const versionInfo = await resolveVersionInfo({ root: tempRoot, git });

    expect(versionInfo.patch).toBe(0);
    expect(versionInfo.version).toBe('1.2.0');
    expect(git.mock.calls.map(([args]) => args[0])).not.toContain('rev-list');
  });

  it('detects the branch from GITHUB_REF_NAME when set', async () => {
    vi.stubEnv('GITHUB_REF_NAME', 'release/1.2');
    const git = createFakeGit();
    const versionInfo = await resolveVersionInfo({ root: tempRoot, git });

    expect(versionInfo.branch).toBe('release/1.2');
    expect(versionInfo.latestTag).toBe('');
    expect(git.mock.calls.map(([args]) => args.join(' '))).not.toContain(
      'rev-parse --abbrev-ref HEAD'
    );
  });
});
