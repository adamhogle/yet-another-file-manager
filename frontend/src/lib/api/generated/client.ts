// AUTO-GENERATED FILE. DO NOT EDIT.
// Source: api/openapi.yaml

import { z } from 'zod';

export class ApiHttpError extends Error {
  status: number;

  constructor(message: string, status: number) {
    super(message);
    this.name = 'ApiHttpError';
    this.status = status;
  }
}

export const DEFAULT_BASE_URL = '';

export const EntryKindSchema = z.enum(['directory', 'file']);
export type EntryKind = z.infer<typeof EntryKindSchema>;

export const DirectoryEntrySchema = z.object({
  canDelete: z.boolean(),
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

export const UserInfoResponseSchema = z.object({
  displayName: z.string(),
  email: z.string().nullable().optional(),
  subject: z.string()
});
export type UserInfoResponse = z.infer<typeof UserInfoResponseSchema>;

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
    // The HTTP status rides the error so callers can distinguish failure
    // classes (the backend's 404 for hidden paths vs other errors). The
    // message alone carries the backend's PublicErrorResponse text, which is
    // deliberately shared between hidden and nonexistent paths.
    throw new ApiHttpError(message, response.status);
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
    // The HTTP status rides the error so callers can distinguish failure
    // classes (the backend's 404 for hidden paths vs other errors). The
    // message alone carries the backend's PublicErrorResponse text, which is
    // deliberately shared between hidden and nonexistent paths.
    throw new ApiHttpError(message, response.status);
  }

  return response.blob();
}

export async function deleteApiV1File(
  baseUrl: string = DEFAULT_BASE_URL,
  query: Record<string, string | number | boolean | null | undefined> = {},
  init: RequestInit = {}
): Promise<void> {
  const searchParams = new URLSearchParams();
  for (const [key, value] of Object.entries(query)) {
    if (value === undefined || value === null || value === '') {
      continue;
    }

    searchParams.set(key, String(value));
  }

  const queryString = searchParams.toString();
  const response = await fetch(`${baseUrl}/api/v1/file${queryString ? `?${queryString}` : ''}`, {
    ...init,
    method: 'DELETE',
    headers: {
      ...(init.headers ?? {})
    }
  });

  if (!response.ok) {
    let message = `Request failed: DELETE /api/v1/file -> ${response.status}`;
    try {
      const body = PublicErrorResponseSchema.parse(await response.json());
      if (body.message) {
        message = body.message;
      }
    } catch {
      // Non-JSON or non-conforming body: keep the fallback message.
    }
    // The HTTP status rides the error so callers can distinguish failure
    // classes (the backend's 404 for hidden paths vs other errors). The
    // message alone carries the backend's PublicErrorResponse text, which is
    // deliberately shared between hidden and nonexistent paths.
    throw new ApiHttpError(message, response.status);
  }
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
    // The HTTP status rides the error so callers can distinguish failure
    // classes (the backend's 404 for hidden paths vs other errors). The
    // message alone carries the backend's PublicErrorResponse text, which is
    // deliberately shared between hidden and nonexistent paths.
    throw new ApiHttpError(message, response.status);
  }

  return HealthResponseSchema.parse(await response.json());
}

export async function getApiV1UsersMe(
  baseUrl: string = DEFAULT_BASE_URL,
  query: Record<string, string | number | boolean | null | undefined> = {},
  init: RequestInit = {}
): Promise<UserInfoResponse> {
  const searchParams = new URLSearchParams();
  for (const [key, value] of Object.entries(query)) {
    if (value === undefined || value === null || value === '') {
      continue;
    }

    searchParams.set(key, String(value));
  }

  const queryString = searchParams.toString();
  const response = await fetch(
    `${baseUrl}/api/v1/users/me${queryString ? `?${queryString}` : ''}`,
    {
      ...init,
      method: 'GET',
      headers: {
        ...(init.headers ?? {})
      }
    }
  );

  if (!response.ok) {
    let message = `Request failed: GET /api/v1/users/me -> ${response.status}`;
    try {
      const body = PublicErrorResponseSchema.parse(await response.json());
      if (body.message) {
        message = body.message;
      }
    } catch {
      // Non-JSON or non-conforming body: keep the fallback message.
    }
    // The HTTP status rides the error so callers can distinguish failure
    // classes (the backend's 404 for hidden paths vs other errors). The
    // message alone carries the backend's PublicErrorResponse text, which is
    // deliberately shared between hidden and nonexistent paths.
    throw new ApiHttpError(message, response.status);
  }

  return UserInfoResponseSchema.parse(await response.json());
}
