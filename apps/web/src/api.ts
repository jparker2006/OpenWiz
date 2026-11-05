// The only module that makes requests: same origin, `/api/v1/...`; GET, and
// the POSTs (below): the send, the create, the new-thread preview, which only
// reads, and the upload; and one DELETE, the removal of a staged copy.
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

// ── Sending (contracts/web-api.v1.md, "Sending (the one write)") ──
// The one write and its two reads. Each answer is handed back as it came;
// src/composer.ts says what it means. Nothing here retries.

export type SendBody = {
  op_id: string;
  mode: 'queue' | 'steer';
  text: string;
  resume_paused_queue: boolean;
  accept_possible_duplicate: boolean;
  /** The ready ids, in the owner's order; absent when there are none, as 0.0.3 sent it. */
  attachments?: string[];
};

/** One answer, as it came: Access's redirect, a response, or none. */
export type Answer =
  | { kind: 'redirect' }
  /** `json` is undefined when the body is not JSON; `empty` when there was no body at all. */
  | { kind: 'response'; status: number; json: unknown; empty: boolean }
  | { kind: 'network' }
  | { kind: 'timeout' }
  | { kind: 'stopped' };

export const SEND_HEADERS: Readonly<Record<string, string>> = {
  ...REQUEST_HEADERS,
  'Content-Type': 'application/json',
  // the custom request header: no cross-site form or simple request carries it.
  'X-OpenWiz-Send': '1',
};

/** The send route. The target is the path's one encoded segment, and only that. */
export function messagesUrl(id: string): string {
  return `${sessionUrl(id)}/messages`;
}

export function previewUrl(id: string, mode: 'queue' | 'steer'): string {
  return `${sessionUrl(id)}/preview?mode=${mode}`;
}

export function lookupUrl(id: string, opId: string): string {
  return `${messagesUrl(id)}/${encodeURIComponent(opId)}`;
}

async function answer(fetchImpl: FetchLike, url: string, init: RequestInit, signal: AbortSignal): Promise<Answer> {
  let res: ResponseLike;
  let body = '';
  try {
    res = await fetchImpl(url, { ...init, credentials: 'same-origin', redirect: 'manual', cache: 'no-store', signal });
    if (res.type === 'opaqueredirect') return { kind: 'redirect' };
    body = await res.text();
  } catch {
    if (signal.aborted) return signal.reason === 'timeout' ? { kind: 'timeout' } : { kind: 'stopped' };
    return { kind: 'network' };
  }
  const json = (res.headers.get('Content-Type') ?? '').toLowerCase().startsWith('application/json') ? parseJson(body) : undefined;
  return { kind: 'response', status: res.status, json, empty: body.trim() === '' };
}

/** The one method name for every POST, kept here so no other module can make one. */
const POST = 'POST';

/** The send: `POST /api/v1/sessions/{id}/messages`. Called once per press; never retried here. */
export function postMessage(fetchImpl: FetchLike, id: string, body: SendBody, signal: AbortSignal): Promise<Answer> {
  return answer(fetchImpl, messagesUrl(id), { method: POST, headers: SEND_HEADERS, body: JSON.stringify(body) }, signal);
}

// ── Creating (contracts/web-api.v1.md, "Creating (the second write)") ──
// The create, its preview, and two reads use the send request headers.
// Each answer is handed back as it came;
// src/launcher.ts says what it means. Nothing here retries.

/** A create's body, and the preview's: `attachments` are the ready ids, in the owner's order. */
export type ThreadBody = { op_id: string; harness: string; model: string; effort: string; text: string; attachments: string[] };

export function optionsUrl(): string {
  return '/api/v1/cloud-options';
}

/** The create route. The project is the path's one encoded segment, and only that. */
export function threadsUrl(projectId: string): string {
  return `/api/v1/projects/${encodeURIComponent(projectId)}/threads`;
}

export function newThreadUrl(opId: string): string {
  return `/api/v1/new-threads/${encodeURIComponent(opId)}`;
}

/** The create: `POST /api/v1/projects/{id}/threads`. Called once per press; never retried here. */
export function postThread(fetchImpl: FetchLike, projectId: string, body: ThreadBody, signal: AbortSignal): Promise<Answer> {
  return answer(fetchImpl, threadsUrl(projectId), { method: POST, headers: SEND_HEADERS, body: JSON.stringify(body) }, signal);
}

/** The new-thread preview: a POST only because it carries the prompt. It reads; it never creates. */
export function postThreadPreview(fetchImpl: FetchLike, projectId: string, body: ThreadBody, signal: AbortSignal): Promise<Answer> {
  return answer(fetchImpl, `${threadsUrl(projectId)}/preview`, { method: POST, headers: SEND_HEADERS, body: JSON.stringify(body) }, signal);
}

/** A read: the preview or the lookup. Never sends. */
export function getAnswer(fetchImpl: FetchLike, url: string, signal: AbortSignal): Promise<Answer> {
  return answer(fetchImpl, url, { method: 'GET', headers: REQUEST_HEADERS }, signal);
}

// ── Attachments ──
// The upload, the list and the removal. Each answer is handed back as it came;
// src/attachments.ts says what it means. Nothing here retries, and no route
// returns a file's bytes: a thumbnail is the browser's own copy.

export function attachmentsUrl(): string {
  return '/api/v1/attachments';
}

/** One staged attachment's removal route. The id is the path's one encoded segment. */
export function attachmentUrl(id: string): string {
  return `${attachmentsUrl()}/${encodeURIComponent(id)}`;
}

/** The upload's headers: the send's checks, a raw body (never multipart), and the name in its own header. */
export const UPLOAD_HEADERS: Readonly<Record<string, string>> = {
  ...REQUEST_HEADERS,
  'Content-Type': 'application/octet-stream',
  'X-OpenWiz-Send': '1',
};

/** The header that carries the file's name, percent-encoded UTF-8. */
export const NAME_HEADER = 'X-OpenWiz-Name';

/** What an upload needs of `XMLHttpRequest`: the one request with progress. */
export interface XhrLike {
  open(method: string, url: string): void;
  setRequestHeader(name: string, value: string): void;
  send(body: Blob): void;
  abort(): void;
  timeout: number;
  readonly status: number;
  readonly responseText: string;
  getResponseHeader(name: string): string | null;
  readonly upload: { onprogress: ((e: ProgressEvent) => unknown) | null };
  onload: ((e: ProgressEvent) => unknown) | null;
  onerror: ((e: ProgressEvent) => unknown) | null;
  ontimeout: ((e: ProgressEvent) => unknown) | null;
  onabort: ((e: ProgressEvent) => unknown) | null;
}

export type XhrFactory = () => XhrLike;

/** The browser's XMLHttpRequest. The only use of it in the app: uploads, for their progress. */
export const browserXhr: XhrFactory = () => new XMLHttpRequest();

/** An upload in flight: its answer, and a way to stop it (the owner's Remove). */
export type Upload = { answer: Promise<Answer>; abort(): void };

/**
 * One upload: `POST /api/v1/attachments`, the file's bytes as they are. Called
 * once per file the owner adds or retries; never retried here. `timeoutMs`
 * beyond the server's deadline: no answer by then is `timeout`.
 */
export function postAttachment(xhr: XhrFactory, file: Blob, name: string, onProgress: (sent: number) => void, timeoutMs: number): Upload {
  const req = xhr();
  let stopped = false;
  const answer = new Promise<Answer>((resolve) => {
    req.open(POST, attachmentsUrl());
    for (const [k, v] of Object.entries(UPLOAD_HEADERS)) req.setRequestHeader(k, v);
    req.setRequestHeader(NAME_HEADER, encodeURIComponent(name));
    req.timeout = timeoutMs;
    req.upload.onprogress = (e) => onProgress(e.loaded);
    req.onload = () => {
      const text = req.responseText;
      const json = (req.getResponseHeader('Content-Type') ?? '').toLowerCase().startsWith('application/json') ? parseJson(text) : undefined;
      // A cross-origin redirect (Access's sign-in page) never loads: it is an error, below.
      resolve({ kind: 'response', status: req.status, json, empty: text.trim() === '' });
    };
    req.onerror = () => resolve(stopped ? { kind: 'stopped' } : { kind: 'network' });
    req.ontimeout = () => resolve({ kind: 'timeout' });
    req.onabort = () => resolve({ kind: 'stopped' });
    req.send(file);
  });
  return {
    answer,
    abort: () => {
      stopped = true;
      req.abort();
    },
  };
}

/** The removal's headers: the send's checks, and no body. */
export const REMOVE_HEADERS: Readonly<Record<string, string>> = { ...REQUEST_HEADERS, 'X-OpenWiz-Send': '1' };

/** One removal: `DELETE /api/v1/attachments/{id}`. Called once per Remove of a ready copy; never retried here. */
export function deleteAttachment(fetchImpl: FetchLike, id: string, signal: AbortSignal): Promise<Answer> {
  return answer(fetchImpl, attachmentUrl(id), { method: 'DELETE', headers: REMOVE_HEADERS }, signal);
}
