const API_BASE = "";

function authHeaders(): Record<string, string> {
  return { "Content-Type": "application/json" };
}

export class UnauthorizedError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "UnauthorizedError";
  }
}

async function handleResponse(res: Response) {
  if (!res.ok) {
    const err = await res.json().catch(() => ({}));
    const message = err.error?.message || `API ${res.status}`;
    if (res.status === 401) {
      window.dispatchEvent(new Event("auth:unauthenticated"));
      throw new UnauthorizedError(message);
    }
    throw new Error(message);
  }
  return res.json();
}

export async function apiGet<T = unknown>(path: string): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    headers: authHeaders(),
    credentials: "include",
  });
  return handleResponse(res);
}

export async function apiPost<T = unknown>(
  path: string,
  body?: unknown,
): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    method: "POST",
    headers: authHeaders(),
    credentials: "include",
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  return handleResponse(res);
}

export async function apiPut<T = unknown>(
  path: string,
  body: unknown,
): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    method: "PUT",
    headers: authHeaders(),
    credentials: "include",
    body: JSON.stringify(body),
  });
  return handleResponse(res);
}

export async function apiPatch<T = unknown>(
  path: string,
  body: unknown,
): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    method: "PATCH",
    headers: authHeaders(),
    credentials: "include",
    body: JSON.stringify(body),
  });
  return handleResponse(res);
}

export async function apiDelete<T = unknown>(path: string): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    method: "DELETE",
    headers: authHeaders(),
    credentials: "include",
  });
  return handleResponse(res);
}
