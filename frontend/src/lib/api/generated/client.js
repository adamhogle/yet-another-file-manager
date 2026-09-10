// AUTO-GENERATED FILE. DO NOT EDIT.
// Source: api/openapi.yaml

export const DEFAULT_BASE_URL = '';

export async function getApiV1Directory(baseUrl = DEFAULT_BASE_URL, query = {}, init = {}) {
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
      const body = await response.json();
      if (body && typeof body.message === 'string' && body.message) {
        message = body.message;
      }
    } catch {
      // Non-JSON body: keep the fallback message.
    }
    throw new Error(message);
  }

  return response.json();
}

export async function getApiV1Download(baseUrl = DEFAULT_BASE_URL, query = {}, init = {}) {
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
      const body = await response.json();
      if (body && typeof body.message === 'string' && body.message) {
        message = body.message;
      }
    } catch {
      // Non-JSON body: keep the fallback message.
    }
    throw new Error(message);
  }

  return response.blob();
}

export async function getApiV1Health(baseUrl = DEFAULT_BASE_URL, query = {}, init = {}) {
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
      const body = await response.json();
      if (body && typeof body.message === 'string' && body.message) {
        message = body.message;
      }
    } catch {
      // Non-JSON body: keep the fallback message.
    }
    throw new Error(message);
  }

  return response.json();
}
