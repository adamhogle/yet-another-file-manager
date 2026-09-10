import { getApiV1Directory } from './generated/client';
import type { DirectoryListing } from './generated/client';

const DEFAULT_BASE_URL = '';
// The OIDC login endpoint the auth gate's 302 redirects browsers to; the
// wrapper sends an expired session through it on a failed directory fetch.
const LOGIN_PATH = '/api/v1/auth/login';

export function toChildPath(currentPath: string, childName: string): string {
  return currentPath ? `${currentPath}/${childName}` : childName;
}

// No 401-specific branch here: under the hybrid auth gate an expired session
// on a download <a> navigation redirects the browser through the OIDC login
// automatically (the gate is path-based: 401 JSON for /api/* fetches, 302 for
// browser navigations), so a download link only needs the plain href.
export function buildDownloadHref(
  currentPath: string,
  fileName: string,
  baseUrl: string = DEFAULT_BASE_URL
): string {
  const params = new URLSearchParams({ p: toChildPath(currentPath, fileName) });
  return `${baseUrl}/api/v1/download?${params.toString()}`;
}

// The generated client throws a plain Error on any non-ok response, with the
// status only inside the message string, so a failed directory fetch cannot
// distinguish 401 from network errors. The probe asks the backend directly:
// a 401 means the session was rejected and the browser should log in again,
// while network errors and other backend failures (503 shared-dir
// unavailable) are not auth errors.
async function isUnauthenticated(): Promise<boolean> {
  try {
    const response = await fetch('/api/v1/health');
    return response.status === 401;
  } catch {
    return false;
  }
}

export async function fetchDirectory(
  relativePath: string = '',
  baseUrl: string = DEFAULT_BASE_URL
): Promise<DirectoryListing> {
  try {
    return await getApiV1Directory(baseUrl, relativePath ? { p: relativePath } : {}, {
      headers: {
        Accept: 'application/json'
      }
    });
  } catch (error) {
    if (await isUnauthenticated()) {
      // The gate rejected the session: send the browser through the OIDC login
      // redirect. A never-resolving promise keeps the caller from rendering an
      // error panel before the navigation happens.
      window.location.assign(LOGIN_PATH);
      return new Promise<DirectoryListing>(() => undefined);
    }
    throw error;
  }
}

export type { DirectoryEntry, DirectoryListing } from './generated/client';
