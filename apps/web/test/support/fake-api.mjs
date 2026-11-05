

import * as at from './attachment-synthetic.mjs';
import * as nt from './newthread-synthetic.ts';
import { DELIVERIES, inventory, lookup, outcome, preview, report } from './synthetic.ts';

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
/** A send that never reached the core: no record, no call, and the connection drops. */
export const LOST = 'lost';

/** A test-only slowdown, to prove a proof doesn't depend on the runner's speed: `OPENWIZ_FAKE_API_SLOW_MS`. */
const SLOW_MS = Number(process.env.OPENWIZ_FAKE_API_SLOW_MS ?? 0) || 0;
/** Only these route kinds (comma-separated, for example `detail,list`); every kind when unset. */
const SLOW_KINDS = process.env.OPENWIZ_FAKE_API_SLOW_KINDS ? process.env.OPENWIZ_FAKE_API_SLOW_KINDS.split(',') : null;

/** The contract's send body cap (the check). */
export const SEND_BODY_CAP = 99_328;
const SEND_KEYS = new Set(['op_id', 'mode', 'text', 'resume_paused_queue', 'accept_possible_duplicate', 'attachments']);
const OP_ID = /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/;

const SEND = /^\/api\/v1\/sessions\/([^/]+)\/messages$/;
const CREATE = /^\/api\/v1\/projects\/([^/]+)\/threads$/;
const CREATE_PREVIEW = /^\/api\/v1\/projects\/([^/]+)\/threads\/preview$/;
const STATUS = /^\/api\/v1\/new-threads\/([^/]+)$/;
const CREATE_KEYS = ['op_id', 'harness', 'model', 'effort', 'text', 'attachments'];
const SETTING = /^[!-,.-~][!-~]{0,127}$/;
const LOOKUP = /^\/api\/v1\/sessions\/([^/]+)\/messages\/([^/]+)$/;
const PREVIEW = /^\/api\/v1\/sessions\/([^/]+)\/preview$/;
const DETAIL = /^\/api\/v1\/sessions\/([^/]+)$/;
const ATTACHMENT = /^\/api\/v1\/attachments\/([^/]+)$/;

/** The route kind of a path. */
function kindOf(path) {
  if (path === '/api/v1/inventory') return 'list';
  if (path === '/api/v1/cloud-options') return 'options';
  if (path === '/api/v1/attachments') return 'attachments';
  if (ATTACHMENT.test(path)) return 'attachment';
  if (CREATE.test(path)) return 'create';
  if (CREATE_PREVIEW.test(path)) return 'create-preview';
  if (STATUS.test(path)) return 'status';
  if (SEND.test(path)) return 'send';
  if (LOOKUP.test(path)) return 'lookup';
  if (PREVIEW.test(path)) return 'preview';
  if (DETAIL.test(path)) return 'detail';
  return 'other';
}

/** The send's checks, in the contract's order, up to the body: a refusal, or null. */
function sendRefusal(req, url, type = /^application\/json\s*(;\s*charset=utf-8\s*)?$/i) {
  const one = (name) => {
    const all = req.headersDistinct?.[name];
    return all && all.length === 1 ? all[0] : null;
  };
  if (url.search !== '') return apiError(400, 'invalid_request', 'the send route takes no query');
  if (one('origin') !== `http://${req.headers.host}` || one('sec-fetch-site') !== 'same-origin') {
    return apiError(403, 'forbidden_origin', 'a send must come from this site');
  }
  if (one('x-openwiz-send') !== '1') return apiError(403, 'missing_custom_header', 'a send needs X-OpenWiz-Send: 1');
  if (type === null) return null;
  if (!type.test(one('content-type') ?? '')) {
    return apiError(415, 'unsupported_media_type', 'a send must be application/json');
  }
  if (req.headers['transfer-encoding'] !== undefined) return apiError(400, 'bad_request', 'Transfer-Encoding is refused');
  const length = one('content-length');
  if (length === null) return apiError(411, 'length_required', 'a send needs Content-Length');
  if (!/^[0-9]+$/.test(length)) return apiError(400, 'bad_request', 'a malformed Content-Length');
  if (Number(length) > SEND_BODY_CAP) return apiError(413, 'body_too_large', 'the body is over the cap');
  return null;
}

/** The create's strict body rules (the preview's too): exactly six keys, `attachments` empty. The body, or null. */
function createBody(text) {
  let body;
  try {
    body = JSON.parse(text);
  } catch {
    return null;
  }
  if (typeof body !== 'object' || body === null || Array.isArray(body)) return null;
  const keys = Object.keys(body);
  if (keys.length !== CREATE_KEYS.length || !keys.every((k) => CREATE_KEYS.includes(k))) return null;
  if (typeof body.op_id !== 'string' || !OP_ID.test(body.op_id)) return null;
  for (const k of ['harness', 'model', 'effort']) if (typeof body[k] !== 'string' || !SETTING.test(body[k])) return null;
  if (typeof body.text !== 'string' || body.text.trim() === '') return null;
  if (!Array.isArray(body.attachments) || !body.attachments.every((a) => typeof a === 'string')) return null;
  return body;
}

/** The strict body rules: the body, or null. */
function sendBody(text) {
  let body;
  try {
    body = JSON.parse(text);
  } catch {
    return null;
  }
  if (typeof body !== 'object' || body === null || Array.isArray(body)) return null;
  if (!Object.keys(body).every((k) => SEND_KEYS.has(k))) return null;
  if (typeof body.op_id !== 'string' || !OP_ID.test(body.op_id)) return null;
  if (body.mode !== 'queue' && body.mode !== 'steer') return null;
  if (typeof body.text !== 'string') return null;
  for (const flag of ['resume_paused_queue', 'accept_possible_duplicate']) if (flag in body && typeof body[flag] !== 'boolean') return null;
  if ('attachments' in body && (!Array.isArray(body.attachments) || !body.attachments.every((a) => typeof a === 'string'))) return null;
  return body;
}

export function fakeApi() {
  const log = [];
  const inFlight = { list: 0, detail: 0, preview: 0, send: 0, lookup: 0, options: 0, create: 0, 'create-preview': 0, status: 0, attachments: 0, attachment: 0, other: 0 };
  const maxInFlight = { ...inFlight };
  const held = [];
  let holding = { list: false, detail: false, preview: false, send: false, lookup: false, options: false, create: false, 'create-preview': false, status: false, attachments: false, attachment: false };
  /** Holds just the requests this picks (for example, only the Steer preview). */
  let holdWhen = () => false;
  let list = () => json(200, inventory([]));
  let detail = (id) => apiError(404, 'not_found', `session ${id} not found`);
  let previewFn = (id, mode) => json(200, preview(id, 'idle', mode));
  // The provider's answer to a new send: a report; { after: DROP | HANG | DROP_HELD, report } when
  // the call happened but its answer never reaches the page; or LOST, a request that never reached the
  // core (no record, no call: the connection drops). A refusal report (possible duplicate, conflicting
  // reuse) leaves no record and makes no call, as the core's do.
  let sendFn = (id, body) => json(200, report(id, body.op_id, body.mode, body.text, outcome(DELIVERIES.sent)));
  let lookupFn = null;
  /** The send record: op id -> { id, mode, text, report }. */
  const records = new Map();
  /** One entry per provider call (the core's `tell`). */
  const calls = [];
  // The new-thread routes .
  let optionsFn = () => json(200, nt.options());
  let createPreviewFn = (project, body) => json(200, nt.preview(project, body));
  // The provider's answer to a new create: a report; { after: DROP | HANG | DROP_HELD, report } when the call
  // happened but its answer never reaches the page; or LOST (no record, no call: the connection drops).
  let createFn = (project, body) => json(200, nt.report(project, body, nt.OUTCOMES.created(`thr_web_9${String(creates.length).padStart(3, '0')}`), nt.start(`thr_web_9${String(creates.length).padStart(3, '0')}`, 'waiting_to_start', body)));
  let statusFn = null;
  /** `fn(project, body, entry)`: a refusal before the core (OpenWiz's own, or Access's), or null to go on. Nothing is recorded or called. */
  let beforeCoreFn = () => null;
  /** The new-thread record: op id -> { project, harness, model, effort, text, report }. */
  const threadRecords = new Map();
  /** One entry per provider call (the core's `spawn`). */
  const creates = [];
  // The attachment routes : the staged copies, by id, and every upload body received.
  const staged = new Map();
  const uploads = [];
  // The core's answer to one upload: by default ready, its kind from its first bytes (the core's
  // rule, for the fixtures); or a reply, or DROP / HANG / LOST.
  let uploadFn = (name, bytes) => at.ready(staged, name, bytes);
  let removeFn = (id) => at.remove(staged, id);
  /** The gate, as the core's: every id staged (and ready), or a refusal naming the first that isn't. */
  const gateRefusal = (ids) => {
    const missing = (ids ?? []).find((id) => !staged.has(id));
    return missing ? apiError(400, 'invalid_request', `attachment ${missing} is not found: it was never staged here, or it was removed or expired; nothing was sent`) : null;
  };

  /** The create route after its checks: a replay, a conflicting reuse, or one call. */
  const createReply = (project, body, entry) => {
    const known = threadRecords.get(body.op_id);
    if (known) {
      const differs = ['project', 'harness', 'model', 'effort', 'text'].filter((k) => (k === 'project' ? known.project !== project : known[k] !== body[k]));
      const refused = (result, status) => json(200, { ...nt.report(project, body, nt.OUTCOMES.failed()), status, provider: null, effects: null, start: null, summary: `${body.harness} · ${body.model} · ${body.effort} requested · ${status}`, result, claimed_at: null, dispatching_at: null, finished_at: null });
      if (differs.length > 0) return refused({ kind: 'conflicting_reuse', differs }, 'conflicting_reuse');
      if (!known.report) return refused({ kind: 'in_progress', state: 'dispatching' }, 'in_progress');
      const r = known.report;
      return json(200, r.result.kind === 'outcome' ? { ...r, result: { ...r.result, replayed: true } } : r);
    }
    const record = { project, harness: body.harness, model: body.model, effort: body.effort, text: body.text, report: null };
    threadRecords.set(body.op_id, record);
    creates.push({ project, op_id: body.op_id, harness: body.harness, model: body.model, effort: body.effort, text: body.text, attachments: body.attachments });
    entry.call = creates.length;
    let reply = createFn(project, body, entry);
    // A created report carries the attachments the core handed over, as the core's does (B2).
    if (body.attachments.length > 0 && reply && typeof reply === 'object' && reply.status === 200) {
      const r = JSON.parse(reply.body);
      reply = json(200, { ...r, attachments: at.promptAttachments(staged, body.attachments) });
    }
    const unclaim = () => {
      threadRecords.delete(body.op_id);
      creates.pop();
      entry.call = null;
    };
    if (reply === LOST) {
      unclaim();
      return DROP;
    }
    if (reply && typeof reply === 'object' && 'after' in reply) {
      record.report = reply.report;
      return reply.after;
    }
    if (reply && typeof reply === 'object' && reply.status === 200) record.report = JSON.parse(reply.body);
    else if (reply && typeof reply === 'object' && ((reply.status >= 400 && reply.status < 500) || reply.status === 503)) unclaim();
    return reply;
  };

  /** The send route after its checks: a replay, a conflicting reuse, or one call. */
  const sendReply = (id, body, entry) => {
    const known = records.get(body.op_id);
    if (known) {
      const differs = ['target', 'mode', 'text'].filter((k) => (k === 'target' ? known.id !== id : known[k] !== body[k]));
      if (differs.length > 0) return json(200, report(id, body.op_id, body.mode, body.text, { kind: 'conflicting_reuse', differs }));
      if (!known.report) return json(200, report(id, body.op_id, body.mode, body.text, { kind: 'in_progress', state: 'dispatching' }));
      const r = known.report;
      return json(200, r.result.kind === 'outcome' ? { ...r, result: { ...r.result, replayed: true } } : r);
    }
    const record = { id, mode: body.mode, text: body.text, report: null };
    records.set(body.op_id, record);
    calls.push({ id, op_id: body.op_id, mode: body.mode, text: body.text, resume_paused_queue: body.resume_paused_queue ?? false, accept_possible_duplicate: body.accept_possible_duplicate ?? false, attachments: body.attachments ?? [] });
    entry.call = calls.length;
    const reply = sendFn(id, body, entry);
    const unclaim = () => {
      records.delete(body.op_id);
      calls.pop();
      entry.call = null;
    };
    if (reply === LOST) {
      unclaim();
      return DROP;
    }
    if (reply && typeof reply === 'object' && 'after' in reply) {
      record.report = reply.report;
      return reply.after;
    }
    if (reply && typeof reply === 'object' && reply.status === 200) {
      const r = JSON.parse(reply.body);
      if (r.status === 'possible_duplicate_refused' || r.status === 'conflicting_reuse') unclaim();
      else record.report = r;
    } else if (reply && typeof reply === 'object' && ((reply.status >= 400 && reply.status < 500) || reply.status === 503)) unclaim();
    return reply;
  };

  const handler = (req, res, url) => {
    const kind = kindOf(url.pathname);
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
    // in flight after its client leaves, as a server run does (F1).
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
    const answer = (reply) => {
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
      if (holding[kind] || holdWhen(entry)) {
        entry.held = true;
        held.push({ kind, send: () => send(reply) });
        return;
      }
      // OPENWIZ_FAKE_API_SLOW_MS: every reply that isn't held comes this much later (real time), as on a slow runner.
      const delay = (reply.delayMs ?? 0) + (SLOW_KINDS === null || SLOW_KINDS.includes(kind) ? SLOW_MS : 0);
      if (delay > 0) setTimeout(() => send(reply), delay);
      else send(reply);
    };
    const method = (allowed) => (req.method === allowed ? null : apiError(405, 'method_not_allowed', 'method not allowed', { Allow: allowed }));
    if (kind === 'create' || kind === 'create-preview') {
      const project = decodeURIComponent((kind === 'create' ? CREATE : CREATE_PREVIEW).exec(url.pathname)[1]);
      const refused = method('POST') ?? sendRefusal(req, url);
      if (refused) return answer(refused);
      const chunks = [];
      req.on('data', (c) => chunks.push(c));
      req.on('end', () => {
        entry.body = Buffer.concat(chunks).toString('utf8');
        const body = createBody(entry.body);
        if (!body) return answer(apiError(400, 'invalid_request', 'the create body breaks the body rules'));
        const refusedBefore = (kind === 'create' ? beforeCoreFn(project, body, entry) : null) ?? gateRefusal(body.attachments);
        if (refusedBefore) return answer(refusedBefore);
        answer(kind === 'create' ? createReply(project, body, entry) : createPreviewFn(project, body, entry));
      });
      return;
    }
    if (kind === 'send') {
      const id = SEND.exec(url.pathname)[1];
      const refused = method('POST') ?? sendRefusal(req, url);
      if (refused) return answer(refused);
      const chunks = [];
      req.on('data', (c) => chunks.push(c));
      req.on('end', () => {
        entry.body = Buffer.concat(chunks).toString('utf8');
        const body = sendBody(entry.body);
        if (!body) return answer(apiError(400, 'invalid_request', 'the send body breaks the body rules'));
        if (body.mode === 'steer' && (body.attachments ?? []).length > 0) return answer(apiError(400, 'invalid_request', 'Steer can\'t carry attachments. Use Queue, or remove them'));
        answer(gateRefusal(body.attachments) ?? sendReply(id, body, entry));
      });
      return;
    }
    if (kind === 'attachments' && req.method === 'POST') {
      const refused = sendRefusal(req, url, /^application\/octet-stream$/i);
      if (refused) return answer(refused);
      const length = req.headers['content-length'];
      if (length === undefined) return answer(apiError(411, 'length_required', 'an upload needs Content-Length'));
      const raw = req.headersDistinct?.['x-openwiz-name'];
      let name = null;
      try {
        name = raw && raw.length === 1 ? decodeURIComponent(raw[0]) : null;
      } catch {
        name = null;
      }
      if (!name) return answer(apiError(400, 'invalid_request', 'an upload needs one X-OpenWiz-Name: the percent-encoded file name'));
      const chunks = [];
      req.on('data', (c) => chunks.push(c));
      req.on('end', () => {
        const bytes = Buffer.concat(chunks);
        entry.name = name;
        entry.size = bytes.length;
        uploads.push({ name, size: bytes.length, head: bytes.subarray(0, 16) });
        const reply = uploadFn(name, bytes, entry);
        answer(reply === LOST ? DROP : reply);
      });
      req.on('error', () => {});
      return;
    }
    if (kind === 'attachment') {
      const refused = method('DELETE') ?? sendRefusal(req, url, null);
      if (refused) return answer(refused);
      return answer(removeFn(decodeURIComponent(ATTACHMENT.exec(url.pathname)[1]), entry));
    }
    let reply;
    if (kind === 'attachments') reply = req.method === 'GET' ? at.listReport(staged) : apiError(405, 'method_not_allowed', 'method not allowed', { Allow: 'GET, POST' });
    else if (kind === 'options') reply = method('GET') ?? optionsFn(entry);
    else if (kind === 'status') {
      const op = decodeURIComponent(STATUS.exec(url.pathname)[1]);
      const known = threadRecords.get(op);
      reply = method('GET') ?? (statusFn ? statusFn(op, known, entry) : json(200, nt.status(op, known?.report ?? null)));
    } else if (kind === 'list') reply = list(url, entry);
    else if (kind === 'detail') reply = method('GET') ?? detail(decodeURIComponent(DETAIL.exec(url.pathname)[1]), url, entry);
    else if (kind === 'preview') {
      const mode = url.searchParams.get('mode') ?? 'queue';
      reply = method('GET') ?? (mode === 'queue' || mode === 'steer' ? previewFn(PREVIEW.exec(url.pathname)[1], mode, entry) : apiError(400, 'invalid_request', 'mode is queue or steer'));
    } else if (kind === 'lookup') {
      const [, id, op] = LOOKUP.exec(url.pathname);
      const known = records.get(op);
      reply =
        method('GET') ??
        (lookupFn
          ? lookupFn(id, op, known, entry)
          : json(200, known && known.id !== id ? { ...lookup(id, op, null), summary: 'other target', result: { kind: 'other_target' } } : lookup(id, op, known?.report ?? null)));
    } else reply = apiError(404, 'no_route', 'no such route');
    answer(reply);
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
    /** `fn(id, mode, entry)` answers previews. */
    setPreview(fn) {
      previewFn = fn;
    },
    /** `fn(id, body, entry)`: the provider's answer to a new send (one call). */
    setSend(fn) {
      sendFn = fn;
    },
    /** `fn(id, opId, record, entry)` answers lookups; by default, the send record. */
    setLookup(fn) {
      lookupFn = fn;
    },
    /** The provider calls, one per new op id that passed every check. */
    calls,
    records,
    /** `fn(entry)` answers the Cloud options. */
    setOptions(fn) {
      optionsFn = fn;
    },
    /** `fn(project, body, entry)` answers new-thread previews. */
    setCreatePreview(fn) {
      createPreviewFn = fn;
    },
    /** `fn(project, body, entry)`: the provider's answer to a new create (one call). */
    setCreate(fn) {
      createFn = fn;
    },
    /** `fn(opId, record, entry)` answers status reads; by default, the new-thread record. */
    setStatus(fn) {
      statusFn = fn;
    },
    /** `fn(project, body, entry)`: refuse a create before the core (a reply), or null to let it through. */
    setBeforeCore(fn) {
      beforeCoreFn = fn;
    },
    /** The provider's creates, one per new op id that passed every check. */
    creates,
    threadRecords,
    /** The staged copies , by id, and every upload body the fake received. */
    staged,
    uploads,
    /** `fn(name, bytes, entry)`: the core's answer to one upload (a reply), or DROP, HANG, LOST. */
    setUpload(fn) {
      uploadFn = fn;
    },
    /** `fn(id, entry)`: the answer to one removal. */
    setRemove(fn) {
      removeFn = fn;
    },
    hold(kind, on = true) {
      holding = { ...holding, [kind]: on };
    },
    /** `fn(entry)`: hold the requests it returns true for, until `release(entry.kind)`. */
    holdWhen(fn) {
      holdWhen = fn;
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
