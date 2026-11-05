// The launcher's state: U7, nothing preselected and only offered
// settings, a switch that empties what the new harness doesn't offer and
// never substitutes; the delivery answer table, as keeps it; U11, everything
// kept on a refusal or an uncertain create, across a reload, and a status read
// that never creates; U12, one op id per attempt, derived, stored before the
// create leaves, so repeats, retries, reloads and copied tabs create at most
// once; and no automatic retry. Synthetic values only.

import assert from 'node:assert/strict';
import { test } from 'node:test';

import type { Answer } from '../../src/api.ts';
import type { StorageLike } from '../../src/composer.ts';
import {
  AUTO_STATUS_READS,
  check,
  checked,
  choose,
  createAnswer,
  edit,
  EMPTY,
  keepOffered,
  Launchers,
  locked,
  NO_CHOICE,
  opIdOf,
  press,
  reconcile,
  restore,
  settle,
  startOver,
} from '../../src/launcher.ts';
import type { LaunchState } from '../../src/launcher.ts';
import * as nt from '../support/newthread-synthetic.ts';
import { FakeClock, FakeFetch, json, settle as flush } from './support.ts';

const PROJECT = 'prj_web_0001';
const TEXT = 'Synthetic prompt: reply ok';
const OPTIONS = nt.options();

let roots = 0;
const nextRoot = () => `web-${String(++roots).padStart(32, '0')}`;
const res = (status: number, value: unknown = undefined, empty = false): Answer => ({ kind: 'response', status, json: value, empty });
const err = (kind: string) => ({ error: { kind, message: `synthetic ${kind}` } });

/** A written draft with all three settings chosen. */
function ready(): LaunchState {
  let s = edit(EMPTY, TEXT, nextRoot);
  s = choose(s, 'harness', 'codex', OPTIONS);
  s = choose(s, 'model', 'syn-codex-model-01', OPTIONS);
  return choose(s, 'effort', 'high', OPTIONS);
}

const body = (s: LaunchState, opId: string) => ({ ...s.choice, op_id: opId, text: s.draft, attachments: [] as [] });

test('U7: nothing is preselected', () => {
  assert.deepEqual(EMPTY.choice, NO_CHOICE);
  assert.deepEqual(NO_CHOICE, { harness: '', model: '', effort: '' });
  // The options mark nothing default, and reading them chooses nothing.
  assert.deepEqual(reconcile(EMPTY, OPTIONS).choice, NO_CHOICE);
});

test('U7: only offered settings; a switch keeps what the new harness offers and empties the rest, visibly, never substituting', () => {
  let s = ready();
  assert.deepEqual(s.choice, { harness: 'codex', model: 'syn-codex-model-01', effort: 'high' });
  // codex's model isn't claude-code's: the model and its effort are emptied, with why.
  const switched = choose(s, 'harness', 'claude-code', OPTIONS);
  assert.deepEqual(switched.choice, { harness: 'claude-code', model: '', effort: '' });
  assert.match(switched.note ?? '', /syn-codex-model-01 isn’t offered for claude-code: choose a model/);
  // A model both harnesses list stays; its effort stays only where the new harness offers it with that model.
  s = choose(choose(s, 'model', 'syn-shared-model', OPTIONS), 'effort', 'high', OPTIONS);
  assert.deepEqual(s.choice, { harness: 'codex', model: 'syn-shared-model', effort: 'high' });
  const kept = choose(s, 'harness', 'claude-code', OPTIONS);
  assert.deepEqual(kept.choice, { harness: 'claude-code', model: 'syn-shared-model', effort: '' });
  assert.match(kept.note ?? '', /high isn’t offered for syn-shared-model: choose an effort/);
  const both = choose(choose(s, 'effort', 'medium', OPTIONS), 'harness', 'claude-code', OPTIONS);
  assert.deepEqual(both.choice, { harness: 'claude-code', model: 'syn-shared-model', effort: 'medium' });
  assert.equal(both.note, null);
  // Nothing outside the options can be chosen, and nothing replaces it.
  assert.deepEqual(choose(EMPTY, 'harness', 'cursor', OPTIONS).choice, NO_CHOICE, 'an unavailable harness');
  assert.deepEqual(choose(ready(), 'model', 'syn-codex-model-99', OPTIONS).choice, { harness: 'codex', model: '', effort: '' });
  assert.deepEqual(keepOffered({ harness: 'codex', model: 'syn-codex-model-01', effort: 'ultra' }, OPTIONS).choice, { harness: 'codex', model: 'syn-codex-model-01', effort: '' });
});

test('U7: options read again drop a choice they no longer offer, visibly', () => {
  const s = ready();
  const fewer = nt.options(nt.options().read_at, { harnesses: [{ ...OPTIONS.harnesses[0]!, models: [{ model: 'syn-codex-model-02', efforts: ['high'] }] }] });
  const r = reconcile(s, fewer);
  assert.deepEqual(r.choice, { harness: 'codex', model: '', effort: '' });
  assert.match(r.note ?? '', /syn-codex-model-01 isn’t offered for codex/);
  assert.deepEqual(reconcile(s, null).choice, NO_CHOICE, 'no options: nothing can stay chosen');
  // Fixed while an attempt may have created a thread.
  const p = press(s);
  assert.ok(p);
  assert.equal(reconcile(p.state, fewer), p.state);
  assert.equal(choose(p.state, 'model', 'syn-codex-model-02', OPTIONS), p.state);
});

test('WS-8 (A-13): "not created" only when the answer proves the request never reached the core', () => {
  const s = ready();
  const op = opIdOf(s.root!, 1);
  const created = nt.report(PROJECT, body(s, op), nt.OUTCOMES.created('thr_web_9001'));
  const cases: [string, Answer, string][] = [
    ['Access redirect', { kind: 'redirect' }, 'signed-out'],
    ['Access 401', res(401), 'signed-out'],
    ['200 report', res(200, created), 'report'],
    ['200 report of another op id', res(200, { ...created, op_id: `${op}x` }), 'unknown'],
    ['200 that is no report', res(200, { schema: 'openwiz.new-thread.v1' }), 'unknown'],
    ...[400, 403, 404, 405, 408, 411, 413, 415, 421, 503].map((st): [string, Answer, string] => [`OpenWiz ${st}`, res(st, err('synthetic')), 'not-created']),
    ['OpenWiz bodyless 503', res(503, undefined, true), 'not-created'],
    ['a 503 page of the network', res(503), 'unknown'],
    ['a 400 with no OpenWiz body', res(400), 'unknown'],
    ['500 record_unavailable', res(500, err('record_unavailable')), 'unknown'],
    ['504 deadline', res(504, err('deadline')), 'unknown'],
    ['502 gateway', res(502), 'unknown'],
    ['the network', { kind: 'network' }, 'unknown'],
    ['45 s', { kind: 'timeout' }, 'unknown'],
  ];
  for (const [what, a, want] of cases) assert.equal(createAnswer(a, op).kind, want, what);
});

test('U12: one op id per attempt, derived from the draft; a repeat while out is no press; Try again is the same op id', () => {
  const s = ready();
  const first = press(s);
  assert.ok(first);
  assert.equal(first.body.op_id, opIdOf(s.root!, 1));
  assert.match(first.body.op_id, /^web-[0-9a-f]{32}-c1$/);
  assert.deepEqual(first.body, { op_id: first.body.op_id, harness: 'codex', model: 'syn-codex-model-01', effort: 'high', text: TEXT, attachments: [] });
  assert.equal(press(first.state), null, 'a double press while the create is out');
  // No answer: unknown. Try again sends the same attempt.
  const lost = settle(first.state, first.body.op_id, createAnswer({ kind: 'network' }, first.body.op_id));
  assert.equal(lost.pending?.state, 'unknown');
  const again = press(lost);
  assert.ok(again);
  assert.equal(again.body.op_id, first.body.op_id, 'Try again: the same op id, so the core replays');
  // A refusal of the retry before the core leaves it unknown, never "not created".
  const refusedRetry = settle(again.state, again.body.op_id, createAnswer(res(503, err('busy')), again.body.op_id));
  assert.equal(refusedRetry.pending?.state, 'unknown');
  // Uncertain: no press at all until the owner starts over.
  const uncertain = settle(again.state, again.body.op_id, createAnswer(res(200, nt.report(PROJECT, again.body, nt.OUTCOMES.uncertain())), again.body.op_id));
  assert.equal(uncertain.pending?.state, 'uncertain');
  assert.equal(press(uncertain), null);
  assert.ok(locked(uncertain));
  const over = startOver(uncertain);
  assert.equal(over.n, 2);
  assert.match(over.note ?? '', /may.*second thread|second thread/);
  assert.equal(press(over)?.body.op_id, opIdOf(s.root!, 2), 'a new attempt is a new op id, and says so');
});

test('after a lost create, a refused Try again leaves the earlier answer shown, and its refusal apart', () => {
  const s = ready();
  const p = press(s)!;
  const lost = settle(p.state, p.body.op_id, createAnswer({ kind: 'network' }, p.body.op_id));
  assert.equal(lost.said?.kind, 'unknown');
  for (const [what, a] of [
    ['an OpenWiz 503', res(503, err('busy'))],
    ['Access 401', res(401)],
    ['Access redirect', { kind: 'redirect' } as Answer],
  ] as const) {
    const again = press(lost)!;
    const refused = settle(again.state, again.body.op_id, createAnswer(a, again.body.op_id));
    assert.equal(refused.pending?.state, 'unknown', `${what}: still unresolved`);
    assert.deepEqual(refused.said, lost.said, `${what}: the earlier answer is what stays shown`);
    assert.match(refused.retry ?? '', /^(busy: synthetic busy|signed out: sign in again, then Try again)$/, `${what}: the retry's refusal, apart`);
    // The next press clears it; a later answer replaces it.
    assert.equal(press(refused)?.state.retry, null);
  }
});

test('U11: a refusal keeps the prompt and settings, and the next press is a new attempt', () => {
  const s = ready();
  const p = press(s)!;
  const refused = settle(p.state, p.body.op_id, createAnswer(res(200, nt.report(PROJECT, p.body, nt.OUTCOMES.notOffered(['syn-codex-model-02']))), p.body.op_id));
  assert.equal(refused.pending, null);
  assert.equal(refused.draft, TEXT);
  assert.deepEqual(refused.choice, s.choice);
  assert.equal(refused.n, 2, 'the refused op id is bound to its outcome');
  assert.equal(refused.said?.kind, 'report');
  // A refusal before the core: the op id was never claimed, so it is used again.
  const q = press(s)!;
  const busy = settle(q.state, q.body.op_id, createAnswer(res(503, err('busy')), q.body.op_id));
  assert.equal(busy.n, 1);
  assert.equal(busy.said?.kind, 'not-created');
});

test('U11: a status read never creates; its own record settles; no record leaves it as it was', () => {
  const s = ready();
  const p = press(s)!;
  const lost = settle(p.state, p.body.op_id, createAnswer({ kind: 'timeout' }, p.body.op_id));
  const c = check(lost);
  assert.ok(c);
  assert.equal(c.opId, p.body.op_id);
  assert.equal(press(c.state), null, 'nothing is pressed while it reads');
  const none = checked(c.state, c.opId, res(200, nt.status(c.opId, null)));
  assert.equal(none.pending?.state, 'unknown');
  assert.match(none.checked ?? '', /no record/);
  const c2 = check(none)!;
  const uncertain = nt.report(PROJECT, p.body, nt.OUTCOMES.uncertain());
  const read = checked(c2.state, c2.opId, res(200, nt.status(c2.opId, uncertain, null, 2)));
  assert.equal(read.pending?.state, 'uncertain');
  assert.equal(read.observations?.threads.length, 2, 'observations, which never settle it');
  assert.equal(read.said?.kind === 'report' && read.said.read, true);
  const c3 = check(read)!;
  const created = nt.report(PROJECT, p.body, nt.OUTCOMES.created('thr_web_9001'));
  const done = checked(c3.state, c3.opId, res(200, nt.status(c3.opId, created, nt.start('thr_web_9001', 'started', p.body))));
  assert.equal(done.created?.report.op_id, p.body.op_id);
  assert.equal(done.created?.start?.stage.kind, 'started');
  assert.equal(done.draft, '', 'the draft is done');
});

test('U11: a reload keeps the draft, the settings and the op id; a create that was out is unknown', () => {
  const s = ready();
  const p = press(s)!;
  const after = restore({ ...EMPTY, draft: p.state.draft, choice: p.state.choice, root: p.state.root, n: p.state.n, pending: p.state.pending });
  assert.equal(after.pending?.state, 'unknown');
  assert.equal(after.said?.kind === 'unknown' && after.said.cause, 'reload');
  assert.equal(press(after)?.body.op_id, p.body.op_id, 'Try again after the reload: the same op id');
});

class MemoryStorage implements StorageLike {
  readonly map = new Map<string, string>();
  getItem(key: string) {
    return this.map.get(key) ?? null;
  }
  setItem(key: string, value: string) {
    this.map.set(key, value);
  }
  removeItem(key: string) {
    this.map.delete(key);
  }
}

function store(storage: StorageLike | null = new MemoryStorage()) {
  const clock = new FakeClock();
  const f = new FakeFetch();
  const launchers = new Launchers({ fetch: f.fetch, storage, timers: clock, newOpId: nextRoot });
  return { clock, f, storage, launchers };
}

const creates = (f: FakeFetch) => f.inits.map((init, i) => ({ init, url: f.urls[i] })).filter((r) => r.init.method === 'POST' && r.url?.endsWith('/threads'));

function fill(l: Launchers) {
  l.edit(PROJECT, TEXT);
  l.choose(PROJECT, 'harness', 'codex', OPTIONS);
  l.choose(PROJECT, 'model', 'syn-codex-model-01', OPTIONS);
  l.choose(PROJECT, 'effort', 'high', OPTIONS);
}

test('U12: the store makes one create per press, stored before it leaves, with the send’s headers', async () => {
  const h = store();
  fill(h.launchers);
  void h.launchers.create(PROJECT);
  void h.launchers.create(PROJECT);
  void h.launchers.create(PROJECT);
  await flush();
  const made = creates(h.f);
  assert.equal(made.length, 1, 'one create for three presses');
  assert.equal(made[0]?.url, `/api/v1/projects/${PROJECT}/threads`);
  assert.deepEqual(made[0]?.init.headers, { Accept: 'application/json', 'X-Requested-With': 'XMLHttpRequest', 'Content-Type': 'application/json', 'X-OpenWiz-Send': '1' });
  const sent = JSON.parse(String(made[0]?.init.body));
  const stored = JSON.parse((h.storage as MemoryStorage).getItem(`openwiz.launcher.${PROJECT}`) ?? 'null');
  assert.equal(opIdOf(stored.root, stored.pending.n), sent.op_id, 'stored before the answer');
  assert.deepEqual(Object.keys(stored).sort(), ['choice', 'draft', 'n', 'pending', 'root'], 'the minimum: never a report');
  // A copied tab (the same storage) derives the same op id.
  const copy = new Launchers({ fetch: new FakeFetch().fetch, storage: h.storage, timers: new FakeClock(), newOpId: nextRoot });
  assert.equal(opIdOf(copy.get(PROJECT).root!, copy.get(PROJECT).pending!.n), sent.op_id);
  h.f.reply(json(200, nt.report(PROJECT, sent, nt.OUTCOMES.created('thr_web_9001'))));
  await flush();
  assert.ok(h.launchers.get(PROJECT).created);
  assert.equal((h.storage as MemoryStorage).getItem(`openwiz.launcher.${PROJECT}`), null, 'nothing is left once created');
});

test('U11, U12: no automatic retry; a timeout or a lost answer creates nothing more', async () => {
  const h = store();
  fill(h.launchers);
  void h.launchers.create(PROJECT);
  await flush();
  await h.clock.advance(45_000);
  await flush();
  assert.equal(h.launchers.get(PROJECT).pending?.state, 'unknown');
  await h.clock.advance(10 * 60_000);
  await flush();
  assert.equal(creates(h.f).length, 1, 'nothing was created again by itself');
  // The page's refresh reads the record, bounded, and never creates.
  for (let i = 0; i < AUTO_STATUS_READS + 5; i++) {
    void h.launchers.follow(PROJECT);
    await flush();
    const pending = h.f.pending.find((p) => !p.done);
    if (pending) h.f.reply(json(200, nt.status(opIdOf(h.launchers.get(PROJECT).root!, 1), null)));
    await flush();
  }
  const reads = h.f.urls.filter((u) => u.startsWith('/api/v1/new-threads/'));
  assert.equal(reads.length, AUTO_STATUS_READS, 'at most A-9’s ten automatic reads');
  assert.equal(creates(h.f).length, 1, 'a status read never creates');
});

test('U12: nothing is created when the op id can’t be stored first', async () => {
  const none = store(null);
  fill(none.launchers);
  void none.launchers.create(PROJECT);
  await flush();
  assert.equal(creates(none.f).length, 0);
  assert.match(none.launchers.blocked(PROJECT) ?? '', /can’t be saved/);
});
