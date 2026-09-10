// AUTO-GENERATED FILE. DO NOT EDIT.
// Source: api/openapi.yaml

import { z } from 'zod';

export const DEFAULT_BASE_URL = '';

export const EntryKindSchema = z.enum(['directory', 'file']);
export type EntryKind = z.infer<typeof EntryKindSchema>;

export const DirectoryEntrySchema = z.object({
  kind: EntryKindSchema,
  modifiedAt: z.string().nullable().optional(),
  name: z.string(),
  sizeBytes: z.number().int().min(0).nullable().optional()
});
export type DirectoryEntry = z.infer<typeof DirectoryEntrySchema>;

export const DirectoryListingSchema = z.object({
  currentPath: z.string(),
  entries: z.array(DirectoryEntrySchema),
  parentPath: z.string().nullable().optional()
});
export type DirectoryListing = z.infer<typeof DirectoryListingSchema>;

export const HealthResponseSchema = z.object({ service: z.string(), status: z.string() });
export type HealthResponse = z.infer<typeof HealthResponseSchema>;

export const PublicErrorResponseSchema = z.object({ message: z.string() });
export type PublicErrorResponse = z.infer<typeof PublicErrorResponseSchema>;

export async function getApiV1Directory(
  baseUrl: string = DEFAULT_BASE_URL,
  query: Record<string, string | number | boolean | null | undefined> = {},
  init: RequestInit = {}
): Promise<DirectoryListing> {
  const searchParams = new URLSearchParams();
  for (const [key, value] of Object.entries(query)) {
    if (value === undefined || value === null || value === '') {
      continue;
    }

    searchParams.set(key, String(value));
  }

  const queryString = searchParams.toString();
  const response = await fetch(
    `${baseUrl}/api/v1/directory${queryString ? `?${queryString}` : ''}`,
    {
      ...init,
      method: 'GET',
      headers: {
        ...(init.headers ?? {})
      }
    }
  );

  if (!response.ok) {
    let message = `Request failed: GET /api/v1/directory -> ${response.status}`;
    try {
      const body = PublicErrorResponseSchema.parse(await response.json());
      if (body.message) {
        message = body.message;
      }
    } catch {
      // Non-JSON or non-conforming body: keep the fallback message.
    }
    throw new Error(message);
  }

  return DirectoryListingSchema.parse(await response.json());
}

export async function getApiV1Download(
  baseUrl: string = DEFAULT_BASE_URL,
  query: Record<string, string | number | boolean | null | undefined> = {},
  init: RequestInit = {}
): Promise<Blob> {
  const searchParams = new URLSearchParams();
  for (const [key, value] of Object.entries(query)) {
    if (value === undefined || value === null || value === '') {
      continue;
    }

    searchParams.set(key, String(value));
  }

  const queryString = searchParams.toString();
  const response = await fetch(
    `${baseUrl}/api/v1/download${queryString ? `?${queryString}` : ''}`,
    {
      ...init,
      method: 'GET',
      headers: {
        ...(init.headers ?? {})
      }
    }
  );

  if (!response.ok) {
    let message = `Request failed: GET /api/v1/download -> ${response.status}`;
    try {
      const body = PublicErrorResponseSchema.parse(await response.json());
      if (body.message) {
        message = body.message;
      }
    } catch {
      // Non-JSON or non-conforming body: keep the fallback message.
    }
    throw new Error(message);
  }

  return response.blob();
}

export async function getApiV1Health(
  baseUrl: string = DEFAULT_BASE_URL,
  query: Record<string, string | number | boolean | null | undefined> = {},
  init: RequestInit = {}
): Promise<HealthResponse> {
  const searchParams = new URLSearchParams();
  for (const [key, value] of Object.entries(query)) {
    if (value === undefined || value === null || value === '') {
      continue;
    }

    searchParams.set(key, String(value));
  }

  const queryString = searchParams.toString();
  const response = await fetch(`${baseUrl}/api/v1/health${queryString ? `?${queryString}` : ''}`, {
    ...init,
    method: 'GET',
    headers: {
      ...(init.headers ?? {})
    }
  });

  if (!response.ok) {
    let message = `Request failed: GET /api/v1/health -> ${response.status}`;
    try {
      const body = PublicErrorResponseSchema.parse(await response.json());
      if (body.message) {
        message = body.message;
      }
    } catch {
      // Non-JSON or non-conforming body: keep the fallback message.
    }
    throw new Error(message);
  }

  return HealthResponseSchema.parse(await response.json());
}
