// The launcher's presentation state, one per project id, and the one place a
// thread is created. Creation uses the same request guards as sending.
//
// - **Nothing is preselected**. A setting is only ever one the core's
// options offer; switching the harness keeps the model and effort only if the
// new harness offers them, and otherwise empties them visibly. Nothing is
// substituted.
// - **Op ids are derived, never minted on events.** A draft gets one random
// root id (`web-` and 32 hex digits) with its first text, stored at once.
// Attempt n's op id is `<root>-c<n>`. A double press, a repeated keyboard
// press, Try again, a reload and a copied tab all derive the same id, so the
// core replays instead of creating twice.
// - **A new attempt only by the owner's word.** After a recorded refusal or
// failure the next press is attempt n + 1 (the old op id is bound to its
// outcome). While an attempt may have created a thread (no answer,
// uncertain, in progress) the prompt and settings are fixed; only "Start
// over" makes attempt n + 1, and it says that this may make a second thread.
// - **Store, then create.** No create leaves unless its attempt is stored in
// this tab's sessionStorage first.
// - **"Not created" only with proof**: Access's 401 or redirect, or
// OpenWiz's own refusal before the core. Everything else unanswered is
// unknown.
// - **No retry**: a status read never creates, and nothing here creates
// without a press.
//
// It computes no option, outcome, effect, stage or retry: the core's answers
// say what happened.

import { getAnswer, newThreadUrl, optionsUrl, postThread, postThreadPreview } from './api.ts';
import type { Answer, FetchLike, ThreadBody } from './api.ts';
import { gate as trayGate, launchTarget, readyIds } from './attachments.ts';
import type { Uploads } from './attachments.ts';
import { readFailure } from './composer.ts';
import type { ErrorWords, StorageLike } from './composer.ts';
import { exact, is, isNull, oneOf, parseApiError, str } from './envelope.ts';
import type { Check } from './envelope.ts';
import { parseOptions, parsePreview, parseReport, parseStatus } from './newthread.ts';
import type { Observations, Options, Preview, Report, StartView, StatusRead } from './newthread.ts';
import type { Timers } from './poller.ts';

/** the launcher gives up waiting at 45 s, beyond the server's 40 s deadline, and shows uncertain. */
export const CREATE_TIMEOUT_MS = 45_000;

/** The configured bound, for the web: at most this many automatic status reads per attempt or created thread; then only Refresh status. */
export const AUTO_STATUS_READS = 10;

export type Field = 'harness' | 'model' | 'effort';

/** The three requested settings; `''` is not chosen. */
export type Choice = { harness: string; model: string; effort: string };
export const NO_CHOICE: Choice = { harness: '', model: '', effort: '' };

export type Unsettled = 'unknown' | 'uncertain' | 'in-progress';

/**
 * The draft's current attempt, while it may have created a thread:
 * - `creating`, `checking`: its create, or its status read, is out (`before`:
 * the state to go back to when that request settles nothing);
 * - `unknown`: no usable answer, so it may or may not have created one;
 * - `uncertain`, `in-progress`: its own record says so.
 */
export type Pending = { n: number; state: 'creating' | 'checking' | Unsettled; before: Unsettled | null };

/** What is stored, and nothing else: the draft, its settings, its root id, the attempt number, and the attempt that may have created a thread. */
export type Stored = { draft: string; choice: Choice; root: string | null; n: number; pending: Pending | null };

/** What the latest answer said about the draft's attempt, in memory only. */
export type Said =
  /** `read`: from a status read (its report is the record, always marked replayed), not the create's own answer. */
  | { kind: 'report'; report: Report; read: boolean }
  | { kind: 'signed-out' }
  | { kind: 'not-created'; status: number; error: ErrorWords | null }
  | { kind: 'unknown'; cause: 'network' | 'timeout' | 'status' | 'body' | 'reload'; status: number | null; error: ErrorWords | null };

/** The new thread's details, once created: its report, and its start as last read. */
export type Created = { report: Report; start: StartView | null; reads: number };

/** A project's launcher: the stored part, plus, in memory only, the latest answer, notes and the created thread. */
export type LaunchState = Stored & {
  said: Said | null;
  /** Threads seen in the project after an uncertain create: observations only. */
  observations: Observations | null;
  /** What the latest status read found, when it settled nothing. */
  checked: string | null;
  /**
   * A Try again refused before the core (OpenWiz's own refusal, or Access's), in words: shown apart from the
   * earlier answer, which stays shown, since the refusal proves nothing about the earlier create.
   */
  retry: string | null;
  /** Why a setting was emptied, or a file refused. */
  note: string | null;
  created: Created | null;
  /** Automatic status reads made for the current attempt. */
  reads: number;
  unsaved: boolean;
};

export const EMPTY: LaunchState = {
  draft: '',
  choice: NO_CHOICE,
  root: null,
  n: 1,
  pending: null,
  said: null,
  observations: null,
  checked: null,
  retry: null,
  note: null,
  created: null,
  reads: 0,
  unsaved: false,
};

/** An attempt's op id: the draft's root and the attempt number. Every tab derives the same. */
export function opIdOf(root: string, n: number): string {
  return `${root}-c${n}`;
}

/** Whether the prompt and settings are fixed: an attempt is out, or may have created a thread. */
export const locked = (s: LaunchState): boolean => s.pending !== null;

/** Whether a request (a create or a status read) is out. */
export const busy = (s: LaunchState): boolean => s.pending?.state === 'creating' || s.pending?.state === 'checking';

/** The current attempt's op id, or null without a root. */
export const currentOpId = (s: LaunchState): string | null => (s.root === null ? null : opIdOf(s.root, s.pending?.n ?? s.n));

// The options, and the choice in them: presentation only, never a rule of the core's.

/** The harness the options offer under this id, or null. */
export const offeredHarness = (options: Options | null, harness: string) => options?.harnesses.find((h) => h.harness === harness) ?? null;

/** The models listed for a harness, in the core's order. */
export const modelsOf = (options: Options | null, harness: string): string[] => offeredHarness(options, harness)?.models.map((m) => m.model) ?? [];

/** The efforts listed for a model of a harness, in the core's order. */
export const effortsOf = (options: Options | null, harness: string, model: string): string[] =>
  offeredHarness(options, harness)?.models.find((m) => m.model === model)?.efforts ?? [];

/**
 * A choice kept only where the options offer it: a harness they list, a model
 * that harness lists, an effort that model lists. Anything else becomes
 * visibly empty, with why; nothing is substituted.
 */
export function keepOffered(choice: Choice, options: Options | null): { choice: Choice; note: string | null } {
  const notes: string[] = [];
  let { harness, model, effort } = choice;
  if (harness !== '' && offeredHarness(options, harness) === null) {
    notes.push(`${harness} isn’t offered now: choose a harness`);
    harness = '';
  }
  if (model !== '' && !modelsOf(options, harness).includes(model)) {
    if (harness !== '') notes.push(`${model} isn’t offered for ${harness}: choose a model`);
    model = '';
  }
  if (effort !== '' && !effortsOf(options, harness, model).includes(effort)) {
    if (model !== '') notes.push(`${effort} isn’t offered for ${model}: choose an effort`);
    effort = '';
  }
  return { choice: { harness, model, effort }, note: notes.length > 0 ? notes.join('; ') : null };
}

/** Choosing one setting: the others stay only where the options offer them with it. */
export function choose(s: LaunchState, field: Field, value: string, options: Options | null): LaunchState {
  if (locked(s) || s.choice[field] === value) return s;
  const { choice, note } = keepOffered({ ...s.choice, [field]: value }, options);
  return { ...s, choice, note };
}

/** After the options are read again: a setting they no longer offer is emptied, visibly. */
export function reconcile(s: LaunchState, options: Options | null): LaunchState {
  if (locked(s)) return s;
  const { choice, note } = keepOffered(s.choice, options);
  if (choice.harness === s.choice.harness && choice.model === s.choice.model && choice.effort === s.choice.effort) return s;
  return { ...s, choice, note };
}

/** Writing the draft. A new draft gets its root id with its first text. Nothing changes while an attempt is unresolved. */
export function edit(s: LaunchState, text: string, mint: () => string): LaunchState {
  if (locked(s) || s.draft === text) return s;
  // A note (why a setting was emptied, a refused file) stays while the owner writes.
  if (s.root !== null || text.trim() === '') return { ...s, draft: text };
  return { ...s, draft: text, root: mint(), n: 1, said: null, checked: null };
}

/** The body of the current attempt, as the core's request takes it. */
export function bodyOf(s: LaunchState, attachments: readonly string[] = []): ThreadBody | null {
  const opId = currentOpId(s);
  if (opId === null) return null;
  return { op_id: opId, harness: s.choice.harness, model: s.choice.model, effort: s.choice.effort, text: s.draft, attachments: [...attachments] };
}

/**
 * A press of Start thread (or Try again): the current attempt, whose op id
 * every tab derives. Null while a request is out, while the attempt is
 * uncertain or in progress, or without text, a root or all three settings.
 */
export function press(s: LaunchState, attachments: readonly string[] = []): { state: LaunchState; body: ThreadBody } | null {
  if (busy(s) || s.root === null || s.draft.trim() === '') return null;
  if (s.choice.harness === '' || s.choice.model === '' || s.choice.effort === '') return null;
  let pending: Pending;
  if (s.pending === null) pending = { n: s.n, state: 'creating', before: null };
  else if (s.pending.state === 'unknown') pending = { ...s.pending, state: 'creating', before: 'unknown' };
  else return null;
  const state: LaunchState = { ...s, pending, checked: null, retry: null, note: null, reads: s.pending === null ? 0 : s.reads };
  const body = bodyOf(state, attachments);
  return body ? { state, body } : null;
}

/**
 * "not created" only when the answer proves the
 * request never reached the core: Access's 401 or redirect, or OpenWiz's own
 * refusal before the core (its JSON error, or its bodyless 503). A 200 is the
 * core's report. Everything else is unknown: the create may have happened. A
 * report for another op id is not this create's answer.
 */
const BEFORE_THE_CORE = new Set([400, 403, 404, 405, 408, 411, 413, 415, 421, 503]);

export function createAnswer(a: Answer, opId: string): Said {
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
        const report = parseReport(a.json);
        return report && report.op_id === opId ? { kind: 'report', report, read: false } : { kind: 'unknown', cause: 'body', status: 200, error: null };
      }
      if (BEFORE_THE_CORE.has(a.status) && (error !== null || (a.status === 503 && a.empty))) return { kind: 'not-created', status: a.status, error };
      return { kind: 'unknown', cause: 'status', status: a.status, error };
    }
  }
}

/** Whether a report is the op id's own record (an outcome, or in progress), not a conflicting reuse. */
const ownRecord = (r: Report): boolean => r.result.kind === 'outcome' || r.result.kind === 'in_progress';

/** Applies the attempt's own record: created opens the details; a refusal or failure frees the draft for attempt n + 1. */
function applyRecord(s: LaunchState, r: Report, start: StartView | null): LaunchState {
  const n = s.pending?.n ?? s.n;
  if (r.result.kind === 'in_progress') return { ...s, pending: { n, state: 'in-progress', before: null } };
  // Not this attempt's record: the op id is bound to something else, so the next press is a new attempt.
  if (r.result.kind === 'conflicting_reuse') return { ...s, pending: null, n: n + 1 };
  switch (r.result.outcome.kind) {
    case 'created':
      // The draft is done: its text, settings and root are gone (nothing is remembered).
      return { ...EMPTY, created: { report: r, start: start ?? r.start, reads: 0 } };
    case 'uncertain':
      return { ...s, pending: { n, state: 'uncertain', before: null } };
    case 'refused':
    case 'failed':
      return { ...s, pending: null, n: n + 1 };
  }
}

/** The answer to a create of `opId`. Only its own record settles it; a refusal speaks for this request only. */
export function settle(s: LaunchState, opId: string, said: Said): LaunchState {
  if (s.pending?.state !== 'creating' || currentOpId(s) !== opId) return s;
  const t: LaunchState = { ...s, said, checked: null };
  if (said.kind === 'report' && ownRecord(said.report)) return applyRecord(t, said.report, null);
  if (said.kind === 'report') return { ...t, pending: null, n: s.pending.n + 1 };
  if (said.kind === 'unknown') return { ...t, pending: { ...s.pending, state: 'unknown', before: null } };
  // Refused before the core claimed it: a new attempt is refused, its number unused.
  const before = s.pending.before;
  if (before === null) return { ...t, pending: null };
  // A Try again refused so proves nothing about the earlier create: it stays as it was, its own answer
  // still shown, and this refusal is shown apart. Unknown stays unknown.
  return { ...s, retry: refusalWords(said), checked: null, pending: { ...s.pending, state: before, before: null } };
}

/** A refusal before the core, in words: Access's sign-out, or OpenWiz's own error. */
function refusalWords(said: Said): string {
  if (said.kind === 'signed-out') return 'signed out: sign in again, then Try again';
  if (said.kind === 'not-created') return said.error ? `${said.error.kind}: ${said.error.message}` : `HTTP ${said.status}`;
  return 'refused';
}

/** Refresh status: a status read of the unresolved attempt. Never creates. */
export function check(s: LaunchState): { state: LaunchState; opId: string } | null {
  const p = s.pending;
  const opId = currentOpId(s);
  if (!p || opId === null || busy(s)) return null;
  if (p.state !== 'unknown' && p.state !== 'uncertain' && p.state !== 'in-progress') return null;
  return { state: { ...s, pending: { ...p, state: 'checking', before: p.state }, checked: null }, opId };
}

/** The answer to a status read of `opId`: its recorded report settles it as the create's would; anything else changes nothing but the note. */
export function checked(s: LaunchState, opId: string, a: Answer): LaunchState {
  const p = s.pending;
  if (p?.state !== 'checking' || currentOpId(s) !== opId) return s;
  const back: LaunchState = { ...s, pending: { ...p, state: p.before ?? 'unknown', before: null } };
  const status = a.kind === 'response' && a.status === 200 ? parseStatus(a.json) : null;
  if (status && status.op_id === opId) {
    if (status.result.kind === 'no_record') {
      return { ...back, checked: 'checked: no record of this create yet. One still on its way can arrive; Try again creates at most once' };
    }
    const r = status.result.report;
    if (r.op_id === opId && ownRecord(r)) {
      const applied = applyRecord({ ...back, said: { kind: 'report', report: r, read: true } }, r, status.result.start);
      return { ...applied, observations: status.result.observations, checked: null, retry: null };
    }
    return { ...back, checked: 'couldn’t check: the answer was not this create' };
  }
  return { ...back, checked: `couldn’t check: ${readFailure(a)}` };
}

/** A status read of the created thread: its start, as read now. Read-only. */
export function startRead(s: LaunchState, status: StatusRead): LaunchState {
  const c = s.created;
  if (!c || status.op_id !== c.report.op_id || status.result.kind !== 'recorded') return s;
  return { ...s, created: { ...c, start: status.result.start ?? c.start } };
}

/**
 * Start over: the owner's explicit new attempt while the last may have
 * created a thread. Its number is retired first, so nothing is ever created
 * again under it; the prompt and settings stay, free to change.
 */
export function startOver(s: LaunchState): LaunchState {
  const p = s.pending;
  if (!p || busy(s)) return s;
  return { ...s, pending: null, n: p.n + 1, said: null, observations: null, checked: null, retry: null, reads: 0, note: 'a new attempt, with a new op id: if the earlier create happened, this one makes a second thread' };
}

/** After a reload: a create or status read that was out has no answer. */
export function restore(s: LaunchState): LaunchState {
  const p = s.pending;
  if (p?.state === 'creating') return { ...s, pending: { ...p, state: 'unknown', before: null }, said: { kind: 'unknown', cause: 'reload', status: null, error: null } };
  if (p?.state === 'checking') return { ...s, pending: { ...p, state: p.before ?? 'unknown', before: null } };
  return s;
}

/** A Cloud options read: the core's options, or why there are none to show. */
export type OptionsSaid = { kind: 'ok'; options: Options } | { kind: 'failed'; why: string };

export function optionsAnswer(a: Answer): OptionsSaid {
  const options = a.kind === 'response' && a.status === 200 ? parseOptions(a.json) : null;
  return options ? { kind: 'ok', options } : { kind: 'failed', why: readFailure(a) };
}

/** A new-thread preview read: the core's preview of exactly this body, or why there is none. */
export type PreviewSaid = { kind: 'ok'; preview: Preview } | { kind: 'failed'; why: string };

export function previewAnswer(a: Answer, body: ThreadBody): PreviewSaid {
  const preview = a.kind === 'response' && a.status === 200 ? parsePreview(a.json) : null;
  const same = preview !== null && preview.op_id === body.op_id && preview.requested.harness === body.harness && preview.requested.model === body.model && preview.requested.effort === body.effort;
  if (preview && same) return { kind: 'ok', preview };
  return { kind: 'failed', why: preview ? 'the answer was not this preview' : readFailure(a) };
}

// Storage: this tab's sessionStorage only, handed in by the page from src/composer.ts.

const KEY = 'openwiz.launcher.';

/** Why nothing can be created when the op id can't be stored first. */
export const UNSAVED = 'the op id can’t be saved in this tab (browser storage is full or off)';

const rootCheck: Check = (v) => typeof v === 'string' && /^web-[0-9a-f]{32}$/.test(v);
const number: Check = (v) => typeof v === 'number' && Number.isSafeInteger(v) && v >= 1;
const unsettled = is('unknown', 'uncertain', 'in-progress');
const storedCheck: Check = exact({
  draft: str,
  choice: exact({ harness: str, model: str, effort: str }),
  root: oneOf(isNull, rootCheck),
  n: number,
  pending: oneOf(isNull, exact({ n: number, state: oneOf(is('creating', 'checking'), unsettled), before: oneOf(isNull, unsettled) })),
});
const isStored = (v: unknown): v is Stored => storedCheck(v);

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

export type LauncherDeps = {
  fetch: FetchLike;
  storage: StorageLike | null;
  timers: Timers;
  newOpId(): string;
  /** Each project's attachment tray: its ready ids go with a create, and it is emptied once they are handed over. */
  uploads?: Uploads;
};

/** Every project's launcher, and the one place a thread is created. */
export class Launchers {
  private readonly deps: LauncherDeps;
  private readonly storable: boolean;
  private readonly states = new Map<string, LaunchState>();
  private readonly listeners = new Set<() => void>();

  constructor(deps: LauncherDeps) {
    this.deps = deps;
    this.storable = writable(deps.storage);
  }

  get(project: string): LaunchState {
    const known = this.states.get(project);
    if (known) return known;
    const state = restore(this.load(project));
    this.states.set(project, state);
    return state;
  }

  /** Why creating is off for `project` because its op id can't be stored, or null. */
  blocked(project: string): string | null {
    return !this.storable || this.get(project).unsaved ? UNSAVED : null;
  }

  /** Why creating is off for `project` because an attachment isn't ready (the server's gate, mirrored), or null. */
  attachmentsOff(project: string): string | null {
    const uploads = this.deps.uploads;
    return uploads ? trayGate(uploads.get(launchTarget(project))) : null;
  }

  /** The ready ids a create in `project` carries now, in the owner's order. */
  attachments(project: string): string[] {
    const uploads = this.deps.uploads;
    return uploads ? readyIds(uploads.get(launchTarget(project))) : [];
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  edit(project: string, text: string): void {
    this.update(project, (s) => edit(s, text, () => this.deps.newOpId()));
  }

  choose(project: string, field: Field, value: string, options: Options | null): void {
    this.update(project, (s) => choose(s, field, value, options));
  }

  reconcile(project: string, options: Options | null): void {
    this.update(project, (s) => reconcile(s, options));
  }

  note(project: string, note: string | null): void {
    this.update(project, (s) => (s.note === note ? s : { ...s, note }));
  }

  startOver(project: string): void {
    this.update(project, startOver);
  }

  /** Closes the created thread's details: the launcher is empty again. */
  closeCreated(project: string): void {
    this.update(project, (s) => (s.created ? { ...s, created: null, said: null } : s));
  }

  /**
   * The one create: `project`, the shown settings and prompt, once. The
   * attempt is stored before the request leaves; if it can't be, nothing is
   * created.
   */
  async create(project: string): Promise<void> {
    if (this.attachmentsOff(project) !== null) return;
    const ids = this.attachments(project);
    const next = press(this.get(project), ids);
    if (!next) return;
    if (!this.storable || !this.save(project, next.state)) {
      this.states.set(project, { ...this.get(project), unsaved: true });
      this.notify();
      return;
    }
    this.states.set(project, { ...next.state, unsaved: false });
    this.notify();
    const a = await this.within((signal) => postThread(this.deps.fetch, project, next.body, signal));
    this.put(project, settle(this.get(project), next.body.op_id, createAnswer(a, next.body.op_id)));
    this.handOver(project);
  }


  private handOver(project: string): void {
    const uploads = this.deps.uploads;
    if (!uploads || !this.get(project).created) return;
    const tray = uploads.get(launchTarget(project));
    if (tray.items.length > 0) uploads.handOver(launchTarget(project), tray.items.length);
  }

  /** Refresh status: the record of the unresolved attempt (`auto`: one of the page's refresh reads, bounded). Never creates. */
  async check(project: string, auto = false): Promise<void> {
    const s = this.get(project);
    if (auto && s.reads >= AUTO_STATUS_READS) return;
    const next = check(s);
    if (!next) return;
    this.put(project, { ...next.state, reads: next.state.reads + (auto ? 1 : 0) });
    const a = await this.within((signal) => getAnswer(this.deps.fetch, newThreadUrl(next.opId), signal));
    this.put(project, checked(this.get(project), next.opId, a));
    this.handOver(project);
  }

  /** The created thread's start, read again (`auto`: bounded, as `check`). Read-only. */
  async readStart(project: string, auto = false): Promise<void> {
    const c = this.get(project).created;
    if (!c || (auto && c.reads >= AUTO_STATUS_READS)) return;
    this.states.set(project, { ...this.get(project), created: { ...c, reads: c.reads + (auto ? 1 : 0) } });
    this.notify();
    const a = await this.within((signal) => getAnswer(this.deps.fetch, newThreadUrl(c.report.op_id), signal));
    const status = a.kind === 'response' && a.status === 200 ? parseStatus(a.json) : null;
    if (status) this.put(project, startRead(this.get(project), status));
  }

  /** 0.0.2's refresh: the unresolved attempt's record, read again, at most AUTO_STATUS_READS times. Never creates. */
  follow(project: string): Promise<void> {
    return this.check(project, true);
  }

  /** 0.0.2's refresh: the created thread's start, read again, at most AUTO_STATUS_READS times. Read-only. */
  followStart(project: string): Promise<void> {
    return this.readStart(project, true);
  }

  /** The Cloud options: a read, never a create. */
  async options(signal: AbortSignal): Promise<OptionsSaid | null> {
    const a = await getAnswer(this.deps.fetch, optionsUrl(), signal);
    return a.kind === 'stopped' ? null : optionsAnswer(a);
  }

  /** What the current attempt would do: the core's preview. A read, never a create. */
  async preview(project: string, signal: AbortSignal): Promise<{ body: ThreadBody; said: PreviewSaid } | null> {
    const body = bodyOf(this.get(project), this.attachments(project));
    if (!body) return null;
    const a = await postThreadPreview(this.deps.fetch, project, body, signal);
    return a.kind === 'stopped' ? null : { body, said: previewAnswer(a, body) };
  }

  private update(project: string, fn: (s: LaunchState) => LaunchState): void {
    const s = this.get(project);
    const next = fn(s);
    if (next !== s) this.put(project, next);
  }

  private async within(run: (signal: AbortSignal) => Promise<Answer>): Promise<Answer> {
    const ctl = new AbortController();
    const t = this.deps.timers.setTimeout(() => ctl.abort('timeout'), CREATE_TIMEOUT_MS);
    try {
      return await run(ctl.signal);
    } finally {
      this.deps.timers.clearTimeout(t);
    }
  }

  private load(project: string): LaunchState {
    try {
      const raw = this.deps.storage?.getItem(KEY + project) ?? null;
      const json: unknown = raw === null ? null : JSON.parse(raw);
      return isStored(json) ? { ...EMPTY, draft: json.draft, choice: json.choice, root: json.root, n: json.n, pending: json.pending } : EMPTY;
    } catch {
      return EMPTY;
    }
  }

  /** Stores the minimum: true when it is stored. */
  private save(project: string, s: LaunchState): boolean {
    const storage = this.deps.storage;
    if (!storage) return false;
    try {
      // A draft without a root (none yet, or created) stores nothing.
      if (s.root === null) storage.removeItem(KEY + project);
      else storage.setItem(KEY + project, JSON.stringify({ draft: s.draft, choice: s.choice, root: s.root, n: s.n, pending: s.pending } satisfies Stored));
      return true;
    } catch {
      return false;
    }
  }

  private put(project: string, state: LaunchState): void {
    this.states.set(project, { ...state, unsaved: !this.save(project, state) });
    this.notify();
  }

  private notify(): void {
    for (const listener of [...this.listeners]) listener();
  }
}
