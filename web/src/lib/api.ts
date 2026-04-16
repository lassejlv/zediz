export type ApiErrorCode =
  | 'unauthorized'
  | 'forbidden'
  | 'not_found'
  | 'conflict'
  | 'validation'
  | 'internal';

export class ApiError extends Error {
  readonly status: number;
  readonly code: ApiErrorCode | string;

  constructor(status: number, code: string, message: string) {
    super(message);
    this.status = status;
    this.code = code;
  }
}

interface ErrorBody {
  error?: { code?: string; message?: string };
}

type Method = 'GET' | 'POST' | 'PATCH' | 'DELETE' | 'PUT';

export interface ApiRequest {
  method?: Method;
  body?: unknown;
  signal?: AbortSignal;
}

export async function api<T>(path: string, opts: ApiRequest = {}): Promise<T> {
  const res = await fetch(`/api/v1${path}`, {
    method: opts.method ?? 'GET',
    headers: opts.body !== undefined ? { 'content-type': 'application/json' } : undefined,
    body: opts.body !== undefined ? JSON.stringify(opts.body) : undefined,
    credentials: 'include',
    signal: opts.signal,
  });

  if (res.status === 204) {
    return undefined as T;
  }

  const text = await res.text();
  const parsed: unknown = text ? safeJson(text) : null;

  if (!res.ok) {
    const body = parsed as ErrorBody | null;
    throw new ApiError(
      res.status,
      body?.error?.code ?? 'unknown',
      body?.error?.message ?? res.statusText,
    );
  }

  return (parsed ?? (undefined as unknown)) as T;
}

function safeJson(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}
