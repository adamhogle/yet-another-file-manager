import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { ApiHttpError, deleteFile } from '../frontend/src/lib/api/client';

describe('deleteFile', () => {
  beforeEach(() => {
    vi.stubGlobal('window', {
      location: { assign: vi.fn(), pathname: '/', search: '?p=' }
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('sends the DELETE method with the child path as the p query parameter', async () => {
    const fetchStub = vi.fn().mockResolvedValueOnce({ ok: true, status: 204 });
    vi.stubGlobal('fetch', fetchStub);

    await deleteFile('docs', 'demo.txt');

    const [url, init] = fetchStub.mock.calls[0];
    expect(url).toBe('/api/v1/file?p=docs%2Fdemo.txt');
    expect(init.method).toBe('DELETE');
  });

  it('resolves on a 204 response', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce({ ok: true, status: 204 }));

    await expect(deleteFile('', 'demo.txt')).resolves.toBeUndefined();
  });

  it('rethrows a 403 as an ApiHttpError carrying the backend message', async () => {
    const fetchStub = vi
      .fn()
      .mockResolvedValueOnce({
        ok: false,
        status: 403,
        json: async () => ({ message: 'The requested file cannot be deleted with this account.' })
      })
      .mockResolvedValueOnce({ status: 200 });
    vi.stubGlobal('fetch', fetchStub);

    const error = await deleteFile('', 'demo.txt').catch((caught) => caught);
    expect(error).toBeInstanceOf(ApiHttpError);
    expect(error.message).toBe('The requested file cannot be deleted with this account.');
  });

  it('redirects through the login endpoint when the session was rejected', async () => {
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
    void deleteFile('', 'demo.txt');

    await vi.waitFor(() => {
      expect(window.location.assign).toHaveBeenCalledWith('/api/v1/auth/login?returnTo=%2F%3Fp%3D');
    });
  });
});
