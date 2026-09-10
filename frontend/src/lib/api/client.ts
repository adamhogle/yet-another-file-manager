import { getApiV1Directory } from './generated/client';
import type { DirectoryListing } from './generated/client';

const DEFAULT_BASE_URL = '';

export function toChildPath(currentPath: string, childName: string): string {
  return currentPath ? `${currentPath}/${childName}` : childName;
}

export function buildDownloadHref(
  currentPath: string,
  fileName: string,
  baseUrl: string = DEFAULT_BASE_URL
): string {
  const params = new URLSearchParams({ p: toChildPath(currentPath, fileName) });
  return `${baseUrl}/api/v1/download?${params.toString()}`;
}

export async function fetchDirectory(
  relativePath: string = '',
  baseUrl: string = DEFAULT_BASE_URL
): Promise<DirectoryListing> {
  return getApiV1Directory(baseUrl, relativePath ? { p: relativePath } : {}, {
    headers: {
      Accept: 'application/json'
    }
  });
}

export type { DirectoryEntry, DirectoryListing } from './generated/client';
