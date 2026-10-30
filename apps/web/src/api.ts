// The only module that makes requests: GET, same origin, `/api/v1/...`.
// It turns every response into one outcome, so the page can tell signed out,
// disconnected, busy and the core's own errors apart (V6).

import { parseApiError } from './envelope.ts';
import type { ApiError, Envelope } from './envelope.ts';

export type ErrorBody = ApiError['error'];

export type Failure =
  | { kind: 'signed-out'; status: number | null }
  | { kind: 'disconnected'; cause: 'network' | 'timeout' | 'gateway'; status: number | null }
  | { kind: 'busy'; status: number; retryAfterMs: number | null; error: ErrorBody | null }
  /** A JSON error from the server or the core, or (error null) a response that is none of the above. */
  | { kind: 'error'; status: number; error: ErrorBody | null };

export type Outcome<D> = { kind: 'ok'; envelope: Envelope<D> } | Failure;

/** What `read` needs of a fetch response. */
export interface ResponseLike {
  readonly type: string;
  readonly status: number;
  readonly headers: { get(name: string): string | null };
  text(): Promise<string>;
}

export type FetchLike = (url: string, init: RequestInit) => Promise<ResponseLike>;

/** Why the page aborted a read: its own timeout, or it no longer wants it. */
export type AbortReason = 'timeout' | 'stop';

export const REQUEST_HEADERS: Readonly<Record<string, string>> = {
  Accept: 'application/json',
  // Cloudflare Access answers an expired sub-request with 401 when it carries
  // this header, instead of redirecting it.
  'X-Requested-With': 'XMLHttpRequest',
};

/** The browser's fetch. The only use of it in the app. */
export const browserFetch: FetchLike = (url, init) => fetch(url, init);

/** The list route. `offset` is the only query the page sends; the core's default limit applies. */
export function inventoryUrl(offset: number): string {
  return Number.isSafeInteger(offset) && offset > 0 ? `/api/v1/inventory?offset=${offset}` : '/api/v1/inventory';
}

/** The detail route. The id is one encoded path segment, so it can never add a path or a query. */
export function sessionUrl(id: string): string {
  return `/api/v1/sessions/${encodeURIComponent(id)}`;
}

/** One read. `'stopped'` when the page aborted it with reason `'stop'`. */
export async function read<D>(
  fetchImpl: FetchLike,
  url: string,
  parse: (json: unknown) => Envelope<D> | null,
  signal: AbortSignal,
): Promise<Outcome<D> | 'stopped'> {
  let res: ResponseLike;
  let body = '';
  try {
    res = await fetchImpl(url, {
      method: 'GET',
      headers: REQUEST_HEADERS,
      credentials: 'same-origin',
      redirect: 'manual',
      cache: 'no-store',
      signal,
    });
    if (res.type !== 'opaqueredirect') body = await res.text();
  } catch {
    if (signal.aborted) return signal.reason === 'timeout' ? { kind: 'disconnected', cause: 'timeout', status: null } : 'stopped';
    return { kind: 'disconnected', cause: 'network', status: null };
  }
  return classify(res, body, parse);
}

function classify<D>(res: ResponseLike, body: string, parse: (json: unknown) => Envelope<D> | null): Outcome<D> {
  // Access redirects an expired session to its sign-in page; `redirect: 'manual'` makes that opaque.
  if (res.type === 'opaqueredirect') return { kind: 'signed-out', status: null };
  const status = res.status;
  // Access answers an expired sub-request with 401. A 403
  // is a refusal that signing in again would not fix, so it is not "signed out".
  if (status === 401) return { kind: 'signed-out', status };
  const json = (res.headers.get('Content-Type') ?? '').toLowerCase().startsWith('application/json') ? parseJson(body) : undefined;
  if (status === 200) {
    const envelope = json === undefined ? null : parse(json);
    return envelope ? { kind: 'ok', envelope } : { kind: 'error', status, error: null };
  }
  const error = json === undefined ? null : (parseApiError(json)?.error ?? null);
  // The server's own 503 is its JSON error, or bodyless past both connection
  // caps. A 503 with a page of its own is the network's.
  if (status === 503 && (error || body.trim() === '')) {
    return { kind: 'busy', status, retryAfterMs: retryAfterMs(res.headers.get('Retry-After')), error };
  }
  if (error) return { kind: 'error', status, error };
  // Not the server's JSON: the tunnel's or the network's own error page.
  if (status >= 500) return { kind: 'disconnected', cause: 'gateway', status };
  return { kind: 'error', status, error: null };
}

/**
 * Whether the server is known to be done with a read, so that the next read
 * may start without two running on the Mac (V4). It is, when openwiz-server
 * answered (an envelope, or its own error, except a 504: "a run past [the
 * deadline] keeps its slot until the core returns"), or when Access answered
 * at the edge (401, its redirect, a 403), before the request reached the Mac.
 * Not when the network failed or answered with a page of its own: the request
 * may have reached the server, which may still be running it.
 */
export function serverDone<D>(o: Outcome<D>): boolean {
  switch (o.kind) {
    case 'ok':
    case 'signed-out':
      return true;
    case 'busy':
      // Only the server's own JSON 503, or its bodyless refusal: no run started.
      return true;
    case 'error':
      return o.error !== null ? o.error.kind !== 'deadline' : o.status === 403;
    case 'disconnected':
      return false;
  }
}

function parseJson(body: string): unknown {
  try {
    return JSON.parse(body);
  } catch {
    return undefined;
  }
}

function retryAfterMs(value: string | null): number | null {
  return value !== null && /^[0-9]{1,6}$/.test(value.trim()) ? Number(value.trim()) * 1000 : null;
}
