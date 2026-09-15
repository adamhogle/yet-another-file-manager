import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { fetchDirectory, fetchIdentity } from '../frontend/src/lib/api/client';

// vitest runs with environment: node, so window is undefined; the spec stubs
// globalThis.window so the wrapper's redirect target is assertable.
describe('fetchDirectory 401 auto-recovery', () => {
  beforeEach(() => {
    vi.stubGlobal('window', {
      location: { assign: vi.fn(), pathname: '/docs', search: '?p=docs' }
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('redirects the browser through the login endpoint with the current path as returnTo', async () => {
    // The realistic path: the auth gate rejects the directory request with a
    // 401 JSON body, the generated client throws (the status is only inside
    // the message string), the wrapper probes /api/v1/health and redirects
    // the browser on a 401. The returnTo carries the current location so
    // re-login lands back on the page the user was on.
    const fetchStub = vi
      .fn()
      .mockResolvedValueOnce({
        ok: false,
        status: 401,
        json: async () => ({ message: 'Authentication is required.' })
      })
      .mockResolvedValueOnce({ status: 401 });
    vi.stubGlobal('fetch', fetchStub);

    // Not awaited: the redirect path returns a never-resolving promise by
    // design (it keeps the caller from rendering an error panel before the
    // navigation happens).
    void fetchDirectory('docs');

    await vi.waitFor(() => {
      expect(window.location.assign).toHaveBeenCalledWith(
        '/api/v1/auth/login?returnTo=%2Fdocs%3Fp%3Ddocs'
      );
    });
    expect(fetchStub).toHaveBeenCalledTimes(2);
  });

  it('rethrows the original error when the health probe finds a genuine backend failure', async () => {
    // A 503 shared-dir failure is not an auth error: the original error
    // propagates and the browser must not loop through login.
    const originalError = new Error('Request failed: GET /api/v1/directory -> 503');
    const fetchStub = vi
      .fn()
      .mockRejectedValueOnce(originalError)
      .mockResolvedValueOnce({ status: 503 });
    vi.stubGlobal('fetch', fetchStub);

    await expect(fetchDirectory('docs')).rejects.toBe(originalError);
    expect(window.location.assign).not.toHaveBeenCalled();
  });

  it('rethrows the original error when the health probe succeeds', async () => {
    // A 200 health response next to a failed directory fetch is a genuine
    // backend error, not an expired session.
    const originalError = new Error('Request failed: GET /api/v1/directory -> 500');
    const fetchStub = vi
      .fn()
      .mockRejectedValueOnce(originalError)
      .mockResolvedValueOnce({ status: 200 });
    vi.stubGlobal('fetch', fetchStub);

    await expect(fetchDirectory('docs')).rejects.toBe(originalError);
    expect(window.location.assign).not.toHaveBeenCalled();
  });
});

describe('fetchIdentity 401 auto-recovery', () => {
  beforeEach(() => {
    vi.stubGlobal('window', {
      location: { assign: vi.fn(), pathname: '/docs', search: '' }
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('redirects the browser through the login endpoint when the session is rejected', async () => {
    // The identity fetch rides the same 401 auto-recovery as the directory
    // fetch: a rejected session sends the browser to login with the current
    // path as returnTo.
    const fetchStub = vi
      .fn()
      .mockRejectedValueOnce(new Error('Request failed: GET /api/v1/users/me -> 401'))
      .mockResolvedValueOnce({ status: 401 });
    vi.stubGlobal('fetch', fetchStub);

    void fetchIdentity();

    await vi.waitFor(() => {
      expect(window.location.assign).toHaveBeenCalledWith('/api/v1/auth/login?returnTo=%2Fdocs');
    });
    expect(fetchStub).toHaveBeenCalledTimes(2);
  });

  it('rethrows the original error when the probe finds a genuine backend failure', async () => {
    const originalError = new Error('Request failed: GET /api/v1/users/me -> 503');
    const fetchStub = vi
      .fn()
      .mockRejectedValueOnce(originalError)
      .mockResolvedValueOnce({ status: 503 });
    vi.stubGlobal('fetch', fetchStub);

    await expect(fetchIdentity()).rejects.toBe(originalError);
    expect(window.location.assign).not.toHaveBeenCalled();
  });
});
