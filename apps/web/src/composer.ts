

import { getAnswer, lookupUrl, postMessage, previewUrl } from './api.ts';
import { gate as trayGate, readyIds, threadTarget } from './attachments.ts';
import type { Uploads } from './attachments.ts';
import type { Answer, FetchLike, SendBody } from './api.ts';
import { exact, is, isNull, list, oneOf, parseApiError, str } from './envelope.ts';
import type { ApiError, Check } from './envelope.ts';
import { parseLookup, parsePreview, parseReport } from './messaging.ts';
import type { Mode, Preview, Report } from './messaging.ts';
import type { Timers } from './poller.ts';

/** N-1: the composer gives up waiting at 45 s, beyond the server's 40 s deadline, and shows uncertain. */
export const SEND_TIMEOUT_MS = 45_000;

/** One press: the pressed button's mode, and the flags only their own buttons set. */
export type Press = { mode: Mode; resumePausedQueue: boolean; acceptPossibleDuplicate: boolean };

/**
 * Where an attempt stands:
 * - `sending`, `checking`: its send, or its lookup, is out;
 * - `unknown`: no usable answer, so it may or may not have been sent;
 * - `uncertain`, `in-progress`: its own record says so;
 * - `done`: its own record settled it (queued, sent, failed: `result`);
 * - `refused`: refused before the core claimed it (`result`), so its number is unused, unless a conflicting reuse showed it is taken.
 */
export type Phase = 'sending' | 'checking' | 'unknown' | 'uncertain' | 'in-progress';
export type Unsettled = 'unknown' | 'uncertain' | 'in-progress';
export type State = Phase | 'done' | 'refused';
export type Result = 'queued' | 'sent' | 'failed' | 'unsupported' | 'not-sent' | 'signed-out' | 'duplicate' | 'conflict';

const STATES = ['sending', 'checking', 'unknown', 'uncertain', 'in-progress', 'done', 'refused'] as const;
const UNSETTLED = ['unknown', 'uncertain', 'in-progress'] as const;
const RESULTS = ['queued', 'sent', 'failed', 'unsupported', 'not-sent', 'signed-out', 'duplicate', 'conflict'] as const;
export const isUnsettled = (p: State): p is Unsettled => p === 'unknown' || p === 'uncertain' || p === 'in-progress';
/** Whether an attempt may have been sent and nothing settled it yet (or a request for it is out). */
export const unresolved = (a: Attempt): boolean => a.state !== 'done' && a.state !== 'refused';

/**
 * One attempt to send the draft: attempt `n` in `mode`, whose op id is
 * `opIdOf(root, mode, n)`. Every attempt stays in the draft's list, in order,
 * until the draft is sent and cleared, or the owner forgets them.
 */
export type Attempt = {
  mode: Mode;
  n: number;
  resumePausedQueue: boolean;
  acceptPossibleDuplicate: boolean;
  state: State;
  /** While sending a retry, or checking: the state to go back to when this request settles nothing. */
  before: Unsettled | null;
  /** For a `done` or `refused` attempt: its outcome, in one word (no report is stored). */
  result: Result | null;
};

/** An attempt's key in the composer's memory. */
export const keyOf = (a: { mode: Mode; n: number }): string => `${a.mode}:${a.n}`;

/** The next attempt number per mode: attempt n's successor is n + 1, the same in every tab. */
export type Next = { queue: number; steer: number };
const FIRST: Next = { queue: 1, steer: 1 };

export type ErrorWords = ApiError['error'];

/** What an answer said about one attempt. */
export type Said =
  | { kind: 'report'; report: Report }
  | { kind: 'signed-out' }
  | { kind: 'not-sent'; status: number; error: ErrorWords | null }
  | { kind: 'unknown'; cause: 'network' | 'timeout' | 'status' | 'body' | 'reload'; status: number | null; error: ErrorWords | null };

/** What the latest "Check again" found, when it did not settle its attempt. */
export type Checked = { kind: 'no-record' } | { kind: 'other-target' } | { kind: 'failed'; why: string };

/**
 * What is stored, and nothing else: the draft, its root id, the next attempt
 * numbers, and its attempts. Once the draft is sent (its root is null), its
 * attempts stay in memory only, until a new draft starts.
 */
export type Stored = { draft: string; root: string | null; next: Next; attempts: Attempt[] };

/**
 * A thread's composer: the stored part, plus, in memory only, what each
 * attempt's last answer said (by `keyOf`), the latest check's note, and
 * whether the last save failed.
 */
export type ThreadState = Stored & {
  said: ReadonlyMap<string, Said>;
  checked: { key: string; note: Checked } | null;
  unsaved: boolean;
};

export const EMPTY: ThreadState = { draft: '', root: null, next: FIRST, attempts: [], said: new Map(), checked: null, unsaved: false };

/** An attempt's op id: the draft's root, the mode and the attempt number. A pure function: every tab derives the same. */
export function opIdOf(root: string, mode: Mode, n: number): string {
  return `${root}-${mode === 'queue' ? 'q' : 's'}${n}`;
}

/** Whether the text is fixed: an attempt with it is out, or may have been sent. */
export function locked(s: ThreadState): boolean {
  return s.attempts.some(unresolved);
}

/** Whether a request (a send or a check) is out. */
export function busy(s: ThreadState): boolean {
  return s.attempts.some((a) => a.state === 'sending' || a.state === 'checking');
}

/** A draft's root id: `web-` and 32 lowercase hex digits, minted once, with its first text. */
export function newOpId(random: (bytes: Uint8Array) => void = (b) => crypto.getRandomValues(b)): string {
  const bytes = new Uint8Array(16);
  random(bytes);
  return `web-${[...bytes].map((b) => b.toString(16).padStart(2, '0')).join('')}`;
}

/**
 * the check: "not sent" only when the answer proves the request never reached the
 * core: Access's 401 or redirect, or OpenWiz's own refusal before the core
 * (its JSON error, or its bodyless 503). A `200` is the core's report.
 * Everything else is unknown: the send may have happened. A report for
 * another op id or thread is not this send's answer.
 */
const BEFORE_THE_CORE = new Set([400, 403, 404, 405, 408, 411, 413, 415, 421, 503]);

export function sendAnswer(a: Answer, id: string, opId: string): Said {
  switch (a.kind) {
    case 'redirect':
      return { kind: 'signed-out' };
    case 'network':
    case 'stopped':
      return { kind: 'unknown', cause: 'network', status: null, error: null };
    case 'timeout':
      return { kind: 'unknown', cause: 'timeout', status: null, error: null };
    case 'response': {
      if (a.status === 401) return { kind: 'signed-out' };
      const error = parseApiError(a.json)?.error ?? null;
      if (a.status === 200) {
        // Only this send's report: this op id, to this thread.
        const report = parseReport(a.json);
        return report && report.op_id === opId && report.target.id === id ? { kind: 'report', report } : { kind: 'unknown', cause: 'body', status: 200, error: null };
      }
      if (BEFORE_THE_CORE.has(a.status) && (error !== null || (a.status === 503 && a.empty))) return { kind: 'not-sent', status: a.status, error };
      return { kind: 'unknown', cause: 'status', status: a.status, error };
    }
  }
}

/** Whether a report is the op id's own record (an outcome, or in progress), not a refusal of this attempt before any claim. */
export const ownRecord = (r: Report): boolean => r.result.kind === 'outcome' || r.result.kind === 'in_progress';

/** Whether a said is a refusal of its request before the core claimed it: proof of nothing about earlier requests. */
export const refusal = (said: Said): boolean => said.kind === 'signed-out' || said.kind === 'not-sent' || (said.kind === 'report' && !ownRecord(said.report));

/** A settled report's outcome in one word. */
function resultOf(r: Report): Result {
  if (r.status === 'queued') return 'queued';
  if (r.status === 'accepted') return 'sent';
  return r.result.kind === 'outcome' && r.result.delivery.kind === 'failed' && r.result.delivery.reason === 'unsupported_mode' ? 'unsupported' : 'failed';
}

/** A refusal's outcome in one word. */
function refusedAs(said: Said): Result {
  if (said.kind === 'signed-out') return 'signed-out';
  if (said.kind === 'report') return said.report.status === 'conflicting_reuse' ? 'conflict' : 'duplicate';
  return 'not-sent';
}

const withSaid = (s: ThreadState, a: Attempt, said: Said): ReadonlyMap<string, Said> => new Map(s.said).set(keyOf(a), said);
const replace = (s: ThreadState, a: Attempt, b: Attempt): Attempt[] => s.attempts.map((x) => (x === a ? b : x));
/** Attempt n of a mode is used: its successor is n + 1. Idempotent, so repeated or copied observations agree. */
const used = (next: Next, mode: Mode, n: number): Next =>
  mode === 'queue' ? { ...next, queue: Math.max(next.queue, n + 1) } : { ...next, steer: Math.max(next.steer, n + 1) };

/**
 * When nothing is unresolved and one attempt was queued or sent, the draft is
 * done: its text and root are gone, and its attempts stay in memory only, to
 * be shown until a new draft starts.
 */
function finish(s: ThreadState): ThreadState {
  if (s.attempts.some(unresolved) || !s.attempts.some((a) => a.result === 'queued' || a.result === 'sent')) return s;
  return { ...s, draft: '', root: null, next: FIRST };
}

/** Applies attempt `a`'s own record. It touches no other attempt, and marks `a`'s number used. */
function applyRecord(s: ThreadState, a: Attempt, r: Report): ThreadState {
  const next = used(s.next, a.mode, a.n);
  const b: Attempt =
    r.status === 'uncertain' || r.status === 'in_progress'
      ? { ...a, state: r.status === 'uncertain' ? 'uncertain' : 'in-progress', before: null, result: null }
      : { ...a, state: 'done', before: null, result: resultOf(r) };
  return finish({ ...s, next, attempts: replace(s, a, b) });
}

const find = (s: ThreadState, opId: string, state: State): Attempt | null => {
  const root = s.root;
  return root === null ? null : (s.attempts.find((a) => a.state === state && opIdOf(root, a.mode, a.n) === opId) ?? null);
};

/** The answer to attempt `opId`'s send. Only its own record settles it; a refusal speaks for this request only. */
export function settle(s: ThreadState, opId: string, said: Said): ThreadState {
  const a = find(s, opId, 'sending');
  if (!a) return s;
  const t: ThreadState = { ...s, said: withSaid(s, a, said), checked: null };
  if (said.kind === 'report' && ownRecord(said.report)) return applyRecord(t, a, said.report);
  if (said.kind === 'unknown') return { ...t, attempts: replace(t, a, { ...a, state: 'unknown', before: null }) };
  // A refusal before the core claimed this request: a retry goes back to what it was; a new attempt is refused, its number unused.
  const conflict = said.kind === 'report' && said.report.status === 'conflicting_reuse';
  const next = conflict ? used(t.next, a.mode, a.n) : t.next;
  const b: Attempt = a.before ? { ...a, state: a.before, before: null } : { ...a, state: 'refused', before: null, result: refusedAs(said) };
  return { ...t, next, attempts: replace(t, a, b) };
}

/** The answer to attempt `opId`'s lookup: its recorded report settles it as the send's would; anything else changes nothing but the note. */
export function checked(s: ThreadState, id: string, opId: string, a: Answer): ThreadState {
  const at = find(s, opId, 'checking');
  if (!at) return s;
  const back: ThreadState = { ...s, attempts: replace(s, at, { ...at, state: at.before ?? 'unknown', before: null }) };
  const note = (n: Checked): ThreadState => ({ ...back, checked: { key: keyOf(at), note: n } });
  const lookup = a.kind === 'response' && a.status === 200 ? parseLookup(a.json) : null;
  if (lookup && lookup.op_id === opId && lookup.target === id) {
    switch (lookup.result.kind) {
      case 'recorded': {
        const r = lookup.result.report;
        if (r.op_id === opId && r.target.id === id && ownRecord(r)) return applyRecord({ ...s, said: withSaid(s, at, { kind: 'report', report: r }), checked: null }, at, r);
        return note({ kind: 'failed', why: 'the answer was not this send' });
      }
      case 'no_record':
        return note({ kind: 'no-record' });
      case 'other_target':
        return note({ kind: 'other-target' });
    }
  }
  return note({ kind: 'failed', why: readFailure(a) });
}

/**
 * Writing the draft. A new draft (no root yet) gets its root id with its
 * first text, and the last draft's attempts are dropped. Nothing changes
 * while an attempt is unresolved.
 */
export function edit(s: ThreadState, text: string, mint: () => string): ThreadState {
  if (locked(s) || s.draft === text) return s;
  if (s.root !== null || text.trim() === '') return { ...s, draft: text };
  return { ...s, draft: text, root: mint(), next: FIRST, attempts: [], said: new Map(), checked: null };
}

/**
 * A press of a send button. It sends the mode's next attempt, whose op id
 * every tab derives the same way:
 * - with nothing unresolved: the draft (Send, Queue, Steer; Send anyway
 *   after a possible-duplicate refusal). An attempt refused before any claim
 *   is sent again under its own, unused, number;
 * - when that attempt is in the list with no answer: the same attempt again
 *   ("Try again"), so the core replays;
 * - otherwise only as the owner's explicit duplicate ("Send again"), with
 *   the earlier attempts kept as they are.
 * Null while a send or check is out, or without text and a root id.
 */
export function press(s: ThreadState, pressed: Press, attachments: readonly string[] = []): { state: ThreadState; opId: string; body: SendBody } | null {
  if (busy(s) || s.root === null || s.draft.trim() === '') return null;
  const n = s.next[pressed.mode];
  const opId = opIdOf(s.root, pressed.mode, n);
  const existing = s.attempts.find((a) => a.mode === pressed.mode && a.n === n) ?? null;
  let flags = { resumePausedQueue: pressed.resumePausedQueue, acceptPossibleDuplicate: pressed.acceptPossibleDuplicate };
  let attempts: Attempt[];
  if (existing?.state === 'unknown') {
    flags = { resumePausedQueue: existing.resumePausedQueue, acceptPossibleDuplicate: existing.acceptPossibleDuplicate };
    attempts = replace(s, existing, { ...existing, state: 'sending', before: 'unknown' });
  } else if (existing && existing.state !== 'refused') {
    return null;
  } else {
    if (s.attempts.some(unresolved) && !pressed.acceptPossibleDuplicate) return null;
    const attempt: Attempt = { mode: pressed.mode, n, ...flags, state: 'sending', before: null, result: null };
    attempts = existing ? replace(s, existing, attempt) : [...s.attempts, attempt];
  }
  const body: SendBody = { op_id: opId, mode: pressed.mode, text: s.draft, resume_paused_queue: flags.resumePausedQueue, accept_possible_duplicate: flags.acceptPossibleDuplicate };

  if (attachments.length > 0) body.attachments = [...attachments];
  return { state: { ...s, attempts, checked: null }, opId, body };
}

/** "Check again": a lookup of one unresolved attempt. Never sends. */
export function check(s: ThreadState, key: string): { state: ThreadState; opId: string } | null {
  const a = s.attempts.find((x) => keyOf(x) === key) ?? null;
  if (!a || s.root === null || busy(s) || !isUnsettled(a.state)) return null;
  return { state: { ...s, attempts: replace(s, a, { ...a, state: 'checking', before: a.state }), checked: null }, opId: opIdOf(s.root, a.mode, a.n) };
}

/**
 * "Forget it" forgets the draft's attempts, not their numbers: every number
 * used so far, in every mode, answered or not, is retired first, so the next
 * send is a new attempt and nothing is ever sent again under a forgotten op
 * id. The text and the root stay.
 */
export function forget(s: ThreadState): ThreadState {
  if (busy(s)) return s;
  const next = s.attempts.reduce((acc, a) => used(acc, a.mode, a.n), s.next);
  return { ...s, next, attempts: [], said: new Map(), checked: null };
}

/** After a reload: a send or check that was out has no answer. */
export function restore(s: ThreadState): ThreadState {
  let said = s.said;
  const attempts = s.attempts.map((a): Attempt => {
    if (a.state === 'sending') {
      said = new Map(said).set(keyOf(a), { kind: 'unknown', cause: 'reload', status: null, error: null });
      return { ...a, state: 'unknown', before: null };
    }
    return a.state === 'checking' ? { ...a, state: a.before ?? 'unknown', before: null } : a;
  });
  return { ...s, attempts, said };
}

/** A read's failure, in words. */
export function readFailure(a: Answer): string {
  switch (a.kind) {
    case 'redirect':
      return 'signed out';
    case 'network':
    case 'stopped':
      return 'can’t reach OpenWiz on your Mac';
    case 'timeout':
      return 'no answer in time';
    case 'response': {
      if (a.status === 401) return 'signed out';
      const error = parseApiError(a.json)?.error;
      return error ? `${error.kind}: ${error.message}` : `HTTP ${a.status}`;
    }
  }
}

export type PreviewSaid = { kind: 'ok'; preview: Preview } | { kind: 'failed'; why: string };

/** A preview's answer: the core's preview for this target and mode, or why there is none. */
export function previewAnswer(a: Answer, id: string, mode: Mode): PreviewSaid {
  const preview = a.kind === 'response' && a.status === 200 ? parsePreview(a.json) : null;
  if (preview && preview.target.id === id && preview.requested_mode === mode) return { kind: 'ok', preview };
  return { kind: 'failed', why: preview ? 'the answer was not this preview' : readFailure(a) };
}

// Storage: this tab's sessionStorage only, never localStorage, cookies, the URL or the title (the check).

export interface StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

const KEY = 'openwiz.composer.';

/** Why nothing can be sent when the op id can't be stored first. */
export const UNSAVED = 'the message id can’t be saved in this tab (browser storage is full or off)';

const rootCheck: Check = (v) => typeof v === 'string' && /^web-[0-9a-f]{32}$/.test(v);
const flag: Check = (v) => typeof v === 'boolean';
const number: Check = (v) => typeof v === 'number' && Number.isSafeInteger(v) && v >= 1;
const attemptCheck = exact({
  mode: is('queue', 'steer'),
  n: number,
  resumePausedQueue: flag,
  acceptPossibleDuplicate: flag,
  state: is(...STATES),
  before: oneOf(isNull, is(...UNSETTLED)),
  result: oneOf(isNull, is(...RESULTS)),
});
const storedCheck: Check = exact({ draft: str, root: oneOf(isNull, rootCheck), next: exact({ queue: number, steer: number }), attempts: list(attemptCheck) });
const isStored = (v: unknown): v is Stored => storedCheck(v);

/** The browser's sessionStorage, or null where it is unavailable. The only use of it in the app. */
export function browserStorage(): StorageLike | null {
  try {
    return window.sessionStorage;
  } catch {
    return null;
  }
}

/** Whether `storage` takes a write now. */
function writable(storage: StorageLike | null): boolean {
  if (!storage) return false;
  try {
    storage.setItem(`${KEY}probe`, '1');
    storage.removeItem(`${KEY}probe`);
    return true;
  } catch {
    return false;
  }
}

export type ComposerDeps = {
  fetch: FetchLike;
  storage: StorageLike | null;
  timers: Timers;
  newOpId(): string;
  /** Each thread's attachment tray : its ready ids go with a send, and it is emptied once they are handed over. */
  uploads?: Uploads;
};

/** Every thread's composer state, and the one place a send is made. */
export class Composers {
  private readonly deps: ComposerDeps;
  private readonly storable: boolean;
  private readonly states = new Map<string, ThreadState>();
  private readonly listeners = new Set<() => void>();
  private readonly mint: () => string;

  constructor(deps: ComposerDeps) {
    this.deps = deps;
    this.storable = writable(deps.storage);
    this.mint = () => deps.newOpId();
  }

  get(id: string): ThreadState {
    const known = this.states.get(id);
    if (known) return known;
    const state = restore(this.load(id));
    this.states.set(id, state);
    return state;
  }

  /** Why sending is off for `id` because its op id can't be stored, or null. */
  blocked(id: string): string | null {
    return !this.storable || this.get(id).unsaved ? UNSAVED : null;
  }

  /** Why sending is off for `id` because an attachment isn't ready (the server's gate, mirrored), or null. */
  attachmentsOff(id: string): string | null {
    const uploads = this.deps.uploads;
    return uploads ? trayGate(uploads.get(threadTarget(id))) : null;
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  edit(id: string, text: string): void {
    const s = this.get(id);
    const next = edit(s, text, this.mint);
    if (next !== s) this.put(id, next);
  }

  /** Forgets the attempts, retiring every number they used. The text stays. */
  forget(id: string): void {
    const s = this.get(id);
    const next = forget(s);
    if (next !== s) this.put(id, next);
  }

  /**
   * The one send: the open thread `id`, the pressed button's mode, once. The
   * attempt is stored before the request leaves; if it can't be, nothing is
   * sent.
   */
  async press(id: string, pressed: Press): Promise<void> {
    const uploads = this.deps.uploads;
    if (this.attachmentsOff(id) !== null) return;
    const ids = uploads ? readyIds(uploads.get(threadTarget(id))) : [];
    const next = press(this.get(id), pressed, ids);
    if (!next) return;
    if (!this.storable || !this.save(id, next.state)) {
      this.states.set(id, { ...this.get(id), unsaved: true });
      this.notify();
      return;
    }
    this.states.set(id, { ...next.state, unsaved: false });
    this.notify();
    const a = await this.within((signal) => postMessage(this.deps.fetch, id, next.body, signal));
    this.put(id, settle(this.get(id), next.opId, sendAnswer(a, id, next.opId)));
    this.handOver(id);
  }


  private handOver(id: string): void {
    const uploads = this.deps.uploads;
    if (!uploads || this.get(id).root !== null) return;
    const tray = uploads.get(threadTarget(id));
    if (tray.items.length > 0) uploads.handOver(threadTarget(id), tray.items.length);
  }

  /** "Check again": the lookup of one unresolved attempt (by `keyOf`). Never sends. */
  async check(id: string, key: string): Promise<void> {
    const next = check(this.get(id), key);
    if (!next) return;
    this.put(id, next.state);
    const a = await this.within((signal) => getAnswer(this.deps.fetch, lookupUrl(id, next.opId), signal));
    this.put(id, checked(this.get(id), id, next.opId, a));
    this.handOver(id);
  }

  /** A preview of a send to `id` in `mode`: a read, never a send. */
  async preview(id: string, mode: Mode, signal: AbortSignal): Promise<PreviewSaid | null> {
    const a = await getAnswer(this.deps.fetch, previewUrl(id, mode), signal);
    return a.kind === 'stopped' ? null : previewAnswer(a, id, mode);
  }

  private async within(run: (signal: AbortSignal) => Promise<Answer>): Promise<Answer> {
    const ctl = new AbortController();
    const t = this.deps.timers.setTimeout(() => ctl.abort('timeout'), SEND_TIMEOUT_MS);
    try {
      return await run(ctl.signal);
    } finally {
      this.deps.timers.clearTimeout(t);
    }
  }

  private load(id: string): ThreadState {
    try {
      const raw = this.deps.storage?.getItem(KEY + id) ?? null;
      const json: unknown = raw === null ? null : JSON.parse(raw);
      return isStored(json) ? { ...EMPTY, draft: json.draft, root: json.root, next: json.next, attempts: json.attempts } : EMPTY;
    } catch {
      return EMPTY;
    }
  }

  /** Stores the minimum : true when it is stored. */
  private save(id: string, s: ThreadState): boolean {
    const storage = this.deps.storage;
    if (!storage) return false;
    try {
      // A draft without a root (none yet, or sent) stores nothing: its attempts stay in memory.
      if (s.root === null) storage.removeItem(KEY + id);
      else storage.setItem(KEY + id, JSON.stringify({ draft: s.draft, root: s.root, next: s.next, attempts: s.attempts } satisfies Stored));
      return true;
    } catch {
      return false;
    }
  }

  private put(id: string, state: ThreadState): void {
    this.states.set(id, { ...state, unsaved: !this.save(id, state) });
    this.notify();
  }

  private notify(): void {
    for (const listener of [...this.listeners]) listener();
  }
}
