import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { fetchDirectory } from '../frontend/src/lib/api/client';

// vitest runs with environment: node, so window is undefined; the spec stubs
// globalThis.window so the wrapper's redirect target is assertable.
describe('fetchDirectory 401 auto-recovery', () => {
  beforeEach(() => {
    vi.stubGlobal('window', { location: { assign: vi.fn() } });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('redirects the browser through the login endpoint when the backend rejects the session', async () => {
    // The realistic path: the auth gate rejects the directory request with a
    // 401 JSON body, the generated client throws (the status is only inside
    // the message string), the wrapper probes /api/v1/health and redirects
    // the browser on a 401.
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
      expect(window.location.assign).toHaveBeenCalledWith('/api/v1/auth/login');
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
