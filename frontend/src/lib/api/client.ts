import { deleteApiV1File, getApiV1Directory, getApiV1UsersMe } from './generated/client';
import type { DirectoryListing, UserInfoResponse } from './generated/client';

const DEFAULT_BASE_URL = '';
// The OIDC login endpoint the auth gate's 302 redirects browsers to; the
// wrapper sends an expired session through it on a failed directory fetch,
// carrying the current path as returnTo so the user lands back where they
// were after re-login.
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

// The delete request uses the DELETE method, which is not a CORS simple
// request: a cross-origin attacker would need a preflight the backend never
// approves, and the session cookie is SameSite=Lax so a cross-site fetch does
// not carry it. A 401 means the session was rejected mid-action, so the
// wrapper sends the browser through the OIDC login like the other data
// fetches.
export async function deleteFile(
  currentPath: string,
  fileName: string,
  baseUrl: string = DEFAULT_BASE_URL
): Promise<void> {
  try {
    await deleteApiV1File(
      baseUrl,
      { p: toChildPath(currentPath, fileName) },
      {
        headers: {
          Accept: 'application/json'
        }
      }
    );
  } catch (error) {
    if (await isUnauthenticated()) {
      return redirectToLogin();
    }
    throw error;
  }
}

// The gate rejected the session: send the browser through the OIDC login
// redirect with the current location (path + query) as the returnTo value, so
// re-login lands back on the page the user was on. A never-resolving promise
// keeps the caller from rendering an error panel before the navigation
// happens.
function redirectToLogin(): Promise<never> {
  const returnTo = window.location.pathname + window.location.search;
  const target = `${LOGIN_PATH}?returnTo=${encodeURIComponent(returnTo)}`;
  window.location.assign(target);
  return new Promise<never>(() => undefined);
}

async function isUnauthenticated(): Promise<boolean> {
  try {
    const response = await fetch('/api/v1/health');
    return response.status === 401;
  } catch {
    return false;
  }
}

// The authenticated identity for the user menu: read from the session cookie
// through the backend, no provider contact. A 401 means the session was
// rejected and the browser should log in again.
export async function fetchIdentity(baseUrl: string = DEFAULT_BASE_URL): Promise<UserInfoResponse> {
  try {
    return await getApiV1UsersMe(
      baseUrl,
      {},
      {
        headers: {
          Accept: 'application/json'
        }
      }
    );
  } catch (error) {
    if (await isUnauthenticated()) {
      return redirectToLogin();
    }
    throw error;
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
      // The gate rejected the session: send the browser through the OIDC
      // login redirect with the current path as returnTo.
      return redirectToLogin();
    }
    throw error;
  }
}

export type { DirectoryEntry, DirectoryListing, UserInfoResponse } from './generated/client';
// The error class the generated client throws; re-exported so views can
// instanceof-check the status instead of hand-rolling the error shape.
export { ApiHttpError } from './generated/client';
