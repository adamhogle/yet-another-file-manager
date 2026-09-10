import { getApiV1Directory } from './generated/client';

const DEFAULT_BASE_URL = '';

export function toChildPath(currentPath, childName) {
  return currentPath ? `${currentPath}/${childName}` : childName;
}

export function buildDownloadHref(currentPath, fileName, baseUrl = DEFAULT_BASE_URL) {
  const params = new URLSearchParams({ p: toChildPath(currentPath, fileName) });
  return `${baseUrl}/api/v1/download?${params.toString()}`;
}

export async function fetchDirectory(relativePath = '', baseUrl = DEFAULT_BASE_URL) {
  return getApiV1Directory(baseUrl, relativePath ? { p: relativePath } : {}, {
    headers: {
      Accept: 'application/json'
    }
  });
}
