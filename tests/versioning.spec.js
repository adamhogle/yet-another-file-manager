import { describe, expect, it } from 'vitest';

import {
  buildVersionInfo,
  formatGitHubOutput,
  parseVersionConfig
} from '../scripts/compute-version.mjs';

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
