import { spawn } from 'node:child_process';
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';

import { afterAll, beforeAll, describe, expect, it } from 'vitest';

import { getApiV1Directory, getApiV1Health } from '../frontend/src/lib/api/generated/client.js';

const repoRoot = process.cwd();

let backendProcess;
let tempDirectory;
let baseUrl;

async function runCommand(command, args, options = {}) {
  await new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      stdio: 'inherit',
      ...options
    });

    child.on('error', reject);
    child.on('exit', (code) => {
      if (code === 0) {
        resolve();
        return;
      }

      reject(new Error(`Command failed: ${command} ${args.join(' ')} (exit ${code ?? 'null'})`));
    });
  });
}

async function waitForBackendReady(url) {
  for (let attempt = 0; attempt < 60; attempt += 1) {
    try {
      const health = await getApiV1Health(url);
      if (health?.status === 'ok') {
        return;
      }
    } catch {
      // Continue polling until backend is ready.
    }

    await delay(500);
  }

  throw new Error('Backend did not become ready in time');
}

beforeAll(async () => {
  tempDirectory = await mkdtemp(path.join(os.tmpdir(), 'yafm-integration-'));
  const sharedRoot = path.join(tempDirectory, 'share');
  const projectsDir = path.join(sharedRoot, 'projects');
  const configPath = path.join(tempDirectory, 'yafm.config.yaml');

  await mkdir(projectsDir, { recursive: true });
  await writeFile(path.join(projectsDir, 'demo.txt'), 'hello from integration test');
  const port = 18080;
  baseUrl = `http://127.0.0.1:${port}`;

  await writeFile(
    configPath,
    `sharedRoot: ${sharedRoot}\nshowHidden: false\nlistenAddress: 127.0.0.1\nlistenPort: ${port}\n`
  );

  await runCommand('npm', ['run', 'frontend:build'], { cwd: repoRoot });

  backendProcess = spawn('cargo', ['run', '--bin', 'backend', '--', configPath], {
    cwd: path.join(repoRoot, 'backend'),
    env: process.env,
    stdio: ['ignore', 'pipe', 'pipe']
  });

  backendProcess.stdout.on('data', (chunk) => {
    process.stdout.write(`[backend] ${chunk}`);
  });
  backendProcess.stderr.on('data', (chunk) => {
    process.stderr.write(`[backend] ${chunk}`);
  });

  await waitForBackendReady(baseUrl);
}, 180000);

afterAll(async () => {
  if (backendProcess && !backendProcess.killed) {
    backendProcess.kill('SIGTERM');
    await delay(300);
    if (!backendProcess.killed) {
      backendProcess.kill('SIGKILL');
    }
  }

  if (tempDirectory) {
    await rm(tempDirectory, { recursive: true, force: true });
  }
});

describe('generated client integration', () => {
  it('fetches directory data from the live backend', async () => {
    const listing = await getApiV1Directory(baseUrl, { p: 'projects' });

    expect(listing.currentPath).toBe('projects');
    expect(Array.isArray(listing.entries)).toBe(true);
    expect(listing.entries).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          name: 'demo.txt',
          kind: 'file'
        })
      ])
    );
  });

  it('surfaces backend validation errors for invalid paths', async () => {
    await expect(getApiV1Directory(baseUrl, { p: '../secret' })).rejects.toThrow(
      'Request failed: GET /api/v1/directory -> 404'
    );
  });
});
