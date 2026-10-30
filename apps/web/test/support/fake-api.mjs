// A scripted fake of the web API for the browser proofs. It answers from the
// test's responders, can hold requests until released, and counts requests in
// flight per route kind, from arrival until the response ends or the client
// gives up (V4, V8).

import { inventory } from './synthetic.ts';

const API_HEADERS = {
  'Content-Type': 'application/json',
  'Cache-Control': 'no-store',
  'X-Content-Type-Options': 'nosniff',
  'Referrer-Policy': 'no-referrer',
  'X-Frame-Options': 'DENY',
  'Content-Security-Policy': "default-src 'none'; frame-ancestors 'none'",
  Connection: 'close',
};

export const json = (status, value, extra = {}) => ({ status, headers: { ...API_HEADERS, ...extra }, body: `${JSON.stringify(value, null, 2)}\n` });
export const apiError = (status, kind, message, extra = {}) => json(status, { error: { kind, message } }, extra);
export const html = (status, body, extra = {}) => ({ status, headers: { 'Content-Type': 'text/html; charset=utf-8', ...extra }, body });
export const bare = (status, headers = {}) => ({ status, headers: { Connection: 'close', ...headers }, body: '' });
export const HANG = 'hang';
/** Closes the connection without an answer. */
export const DROP = 'drop';
/**
 * Closes the connection without an answer, but the server goes on working:
 * the request stays in flight until the test releases it (a tunnel that drops
 * while the origin still runs the read).
 */
export const DROP_HELD = 'drop-held';

export function fakeApi() {
  const log = [];
  const inFlight = { list: 0, detail: 0, other: 0 };
  const maxInFlight = { list: 0, detail: 0, other: 0 };
  const held = [];
  let holding = { list: false, detail: false };
  let list = () => json(200, inventory([]));
  let detail = (id) => apiError(404, 'not_found', `session ${id} not found`);

  const handler = (req, res, url) => {
    const kind = url.pathname === '/api/v1/inventory' ? 'list' : url.pathname.startsWith('/api/v1/sessions/') ? 'detail' : 'other';
    const entry = {
      kind,
      method: req.method,
      path: url.pathname,
      search: url.search,
      headers: req.headers,
      startedAt: Date.now(),
      endedAt: null,
      status: null,
      completed: false,
    };
    log.push(entry);
    inFlight[kind]++;
    maxInFlight[kind] = Math.max(maxInFlight[kind], inFlight[kind]);
    // In flight until the server is done with it: its response is sent or, for
    // a request that never gets one, its client gave up. A held request stays
    // in flight after its client leaves, as a server run does.
    const end = () => {
      if (entry.endedAt !== null) return;
      entry.endedAt = Date.now();
      inFlight[kind]--;
    };
    res.on('close', () => {
      entry.clientGone = true;
      if (!entry.held) end();
    });
    const send = (r) => {
      entry.held = false;
      if (res.destroyed || res.writableEnded) return end();
      entry.status = r.status;
      entry.completed = true;
      res.writeHead(r.status, { ...r.headers, 'Content-Length': String(Buffer.byteLength(r.body ?? '')) });
      res.end(r.body ?? '');
      end();
    };
    let reply;
    if (kind === 'list') reply = list(url, entry);
    else if (kind === 'detail') reply = detail(decodeURIComponent(url.pathname.slice('/api/v1/sessions/'.length)), url, entry);
    else reply = apiError(404, 'no_route', 'no such route');
    if (reply === HANG) return;
    if (reply === DROP) {
      req.socket.destroy();
      return;
    }
    if (reply === DROP_HELD) {
      entry.held = true;
      held.push({
        kind,
        send: () => {
          entry.held = false;
          end();
        },
      });
      req.socket.destroy();
      return;
    }
    if (holding[kind]) {
      entry.held = true;
      held.push({ kind, send: () => send(reply) });
      return;
    }
    if (reply.delayMs) setTimeout(() => send(reply), reply.delayMs);
    else send(reply);
  };

  return {
    handler,
    log,
    inFlight,
    maxInFlight,
    /** `fn(url, entry)` answers list reads: a reply, or HANG. */
    setList(fn) {
      list = fn;
    },
    /** `fn(id, url, entry)` answers detail reads. */
    setDetail(fn) {
      detail = fn;
    },
    hold(kind, on = true) {
      holding = { ...holding, [kind]: on };
    },
    /** Sends the held replies of `kind`, oldest first. */
    release(kind) {
      for (let i = 0; i < held.length; ) {
        const h = held[i];
        if (h?.kind === kind) {
          held.splice(i, 1);
          h.send();
        } else i++;
      }
    },
    held: (kind) => held.filter((h) => h.kind === kind).length,
    reads: (kind) => log.filter((e) => e.kind === kind),
  };
}
