import { spawn, type ChildProcess, type ChildProcessByStdio } from 'node:child_process';
import { existsSync } from 'node:fs';
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import type { Readable } from 'node:stream';
import { setTimeout as delay } from 'node:timers/promises';

import { afterAll, beforeAll, describe, expect, it } from 'vitest';

import { getApiV1Directory, getApiV1Health } from '../frontend/src/lib/api/generated/client';

const repoRoot = process.cwd();
const READINESS_BUDGET_MS = 150_000;
const LISTEN_LINE_PATTERN = /backend listening on http:\/\/\S+:(\d+)/;
const ANSI_PATTERN = /\u001B\[[0-9;]*m/g;

// `stdio: ['ignore', 'pipe', 'pipe']` ignores stdin and pipes stdout/stderr, so the
// spawned handle is a ChildProcessByStdio with non-null streams.
type BackendProcess = ChildProcessByStdio<null, Readable, Readable>;

let backendProcess: BackendProcess | undefined;
let backendStdout: string;
let tempDirectory: string;
let baseUrl: string;

function stripAnsiEscapes(text: string): string {
  return text.replace(ANSI_PATTERN, '');
}

async function waitForListenPort(startedAt: number): Promise<number> {
  const deadline = startedAt + READINESS_BUDGET_MS;

  while (Date.now() < deadline) {
    const match = LISTEN_LINE_PATTERN.exec(stripAnsiEscapes(backendStdout));
    if (match) {
      return Number(match[1]);
    }

    await delay(250);
  }

  throw new Error(
    `Backend did not report "backend listening on http://<address>:<port>" within ${READINESS_BUDGET_MS}ms`
  );
}

async function waitForBackendReady(url: string, startedAt: number): Promise<void> {
  const deadline = startedAt + READINESS_BUDGET_MS;

  while (Date.now() < deadline) {
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

  throw new Error(`Backend did not become ready at ${url} within ${READINESS_BUDGET_MS}ms`);
}

function waitForExit(child: ChildProcess, timeoutMs: number): Promise<boolean> {
  return new Promise((resolve) => {
    if (child.exitCode !== null || child.signalCode !== null) {
      resolve(true);
      return;
    }

    const timer = setTimeout(() => resolve(false), timeoutMs);
    child.once('exit', () => {
      clearTimeout(timer);
      resolve(true);
    });
  });
}

function signalBackendGroup(signal: NodeJS.Signals): void {
  // The backend spawns detached as a process-group leader because `cargo run`
  // wraps the actual binary and killing only the wrapper would orphan it. A
  // negative pid signals the whole group so no cargo/rustc child survives.
  try {
    if (backendProcess?.pid) {
      process.kill(-backendProcess.pid, signal);
    }
  } catch {
    // The process group is already gone.
  }
}

beforeAll(async () => {
  const startedAt = Date.now();

  tempDirectory = await mkdtemp(path.join(os.tmpdir(), 'yafm-integration-'));
  const sharedRoot = path.join(tempDirectory, 'share');
  const projectsDir = path.join(sharedRoot, 'projects');
  const configPath = path.join(tempDirectory, 'yafm.config.yaml');

  await mkdir(projectsDir, { recursive: true });
  await writeFile(path.join(projectsDir, 'demo.txt'), 'hello from integration test');

  // listenPort: 0 lets the OS pick a free port; the backend reports the actual
  // assigned port on stdout and the spec connects the client to it.
  await writeFile(
    configPath,
    `sharedRoot: "${sharedRoot}"\nshowHidden: false\nlistenAddress: 127.0.0.1\nlistenPort: 0\n`
  );

  backendProcess = spawn('cargo', ['run', '--bin', 'backend', '--', configPath], {
    cwd: path.join(repoRoot, 'backend'),
    env: process.env,
    detached: true,
    stdio: ['ignore', 'pipe', 'pipe']
  });

  backendStdout = '';
  backendProcess.stdout.on('data', (chunk) => {
    backendStdout += chunk;
    process.stdout.write(`[backend] ${chunk}`);
  });
  backendProcess.stderr.on('data', (chunk) => {
    process.stderr.write(`[backend] ${chunk}`);
  });

  const port = await waitForListenPort(startedAt);
  baseUrl = `http://127.0.0.1:${port}`;

  await waitForBackendReady(baseUrl, startedAt);
}, 180000);

afterAll(async () => {
  if (backendProcess?.pid) {
    signalBackendGroup('SIGTERM');

    if (!(await waitForExit(backendProcess, 3000))) {
      signalBackendGroup('SIGKILL');
      await waitForExit(backendProcess, 3000);
    }
  }

  if (tempDirectory) {
    await rm(tempDirectory, { recursive: true, force: true });
  }
});

const hasFrontendDist = existsSync(path.join(repoRoot, 'frontend', 'dist'));

if (!hasFrontendDist) {
  // vitest suppresses console output for skipped suites, so the reason is written directly.
  process.stderr.write(
    'Skipping the integration spec because frontend/dist is missing. Build it with "npm run frontend:build" and re-run.\n'
  );
}

describe.skipIf(!hasFrontendDist)('generated client integration', () => {
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
      'The requested directory could not be found.'
    );
  });
});
