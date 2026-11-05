

import assert from 'node:assert/strict';
import { test } from 'node:test';

import type { Answer } from '../../src/api.ts';
import { check, checked, Composers, edit, EMPTY, forget, isUnsettled, keyOf, newOpId, opIdOf, press, restore, SEND_TIMEOUT_MS, sendAnswer, settle } from '../../src/composer.ts';
import type { Press, StorageLike, ThreadState } from '../../src/composer.ts';
import { DELIVERIES, lookup, outcome, preview, report } from '../support/synthetic.ts';
import { FakeClock, FakeFetch, json, settle as flush } from './support.ts';

const ID = 'thr_web_0001';
const OP = 'web-0123456789abcdef0123456789abcdef-q1';
const TEXT = 'Synthetic message: reply ok';
const QUEUE: Press = { mode: 'queue', resumePausedQueue: false, acceptPossibleDuplicate: false };
const STEER: Press = { mode: 'steer', resumePausedQueue: false, acceptPossibleDuplicate: false };
const DUPLICATE: Press = { ...QUEUE, acceptPossibleDuplicate: true };

const res = (status: number, value: unknown = undefined, empty = false): Answer => ({ kind: 'response', status, json: value, empty });
const err = (kind: string) => ({ error: { kind, message: `synthetic ${kind}` } });
const sent = report(ID, OP, 'queue', TEXT, outcome(DELIVERIES.sent));
const answer = (a: Answer) => sendAnswer(a, ID, OP);

let roots = 0;
const nextRoot = () => `web-${String(++roots).padStart(32, '0')}`;
/** A written draft, with its root id. */
const draft = (text = TEXT): ThreadState => edit(EMPTY, text, nextRoot);
const earlierSend = [{ op_id: `web-${'e'.repeat(32)}-q1`, mode: 'queue' as const, claimed_at: 1, in_flight: true, reconciliation: { kind: 'not_read' as const } }];
const failedArchived = { kind: 'failed' as const, reason: 'target_archived' as const, text: 'failed: the target is archived, and archived targets are refused' };

/** Presses, then settles that send with `a`. */
function pressAndSettle(s: ThreadState, p: Press, a: Answer) {
  const next = press(s, p);
  assert.ok(next, 'the press goes ahead');
  return { opId: next.opId, state: settle(next.state, next.opId, sendAnswer(a, ID, next.opId)) };
}
const states = (s: ThreadState) => s.attempts.map((a) => `${a.mode[0]}${a.n}:${a.state}`);
/** The key of the oldest attempt that a check can read. */
const open = (s: ThreadState) => {
  const a = s.attempts.find((x) => isUnsettled(x.state));
  return a ? keyOf(a) : '';
};

test('"not sent" only when the answer proves the request never reached the core', () => {
  const cases: [string, Answer, string][] = [
    ['Access redirect', { kind: 'redirect' }, 'signed-out'],
    ['Access 401', res(401, undefined), 'signed-out'],
    ['200 report', res(200, sent), 'report'],
    ['200 that is no report', res(200, { schema: 'openwiz.messaging.v1' }), 'unknown'],
    ['200 HTML', res(200, undefined), 'unknown'],
    ...[400, 403, 404, 405, 408, 411, 413, 415, 421, 503].map((s): [string, Answer, string] => [`OpenWiz ${s}`, res(s, err('synthetic')), 'not-sent']),
    ['OpenWiz bodyless 503', res(503, undefined, true), 'not-sent'],
    ['a 503 page of the network', res(503, undefined), 'unknown'],
    ['Access 403 page', res(403, undefined), 'unknown'],
    ['a 400 with no OpenWiz body', res(400, undefined), 'unknown'],
    ['500 record_unavailable', res(500, err('record_unavailable')), 'unknown'],
    ['500 internal', res(500, err('internal')), 'unknown'],
    ['502 page', res(502, undefined), 'unknown'],
    ['504 deadline', res(504, err('deadline')), 'unknown'],
    ['Cloudflare 530 page', res(530, undefined), 'unknown'],
    ['a status OpenWiz never sends', res(418, err('synthetic')), 'unknown'],
    ['a network error', { kind: 'network' }, 'unknown'],
    ['no answer in 45 s', { kind: 'timeout' }, 'unknown'],
  ];
  for (const [what, a, kind] of cases) assert.equal(answer(a).kind, kind, what);
  assert.equal(sendAnswer(res(200, sent), ID, `web-${'f'.repeat(32)}-q1`).kind, 'unknown', 'a report for another op id');
  assert.equal(sendAnswer(res(200, sent), 'thr_web_0002', OP).kind, 'unknown', 'a report for another thread');
  assert.deepEqual(answer(res(415, err('unsupported_media_type'))), { kind: 'not-sent', status: 415, error: err('unsupported_media_type').error });
});

test('uncertain is sticky: a refused retry of an unknown attempt, by OpenWiz or by the core, leaves it unknown', () => {
  const lost = pressAndSettle(draft(), QUEUE, { kind: 'network' });
  assert.deepEqual(states(lost.state), ['q1:unknown']);
  const refusals: Answer[] = [
    res(503, err('busy')),
    res(401),
    { kind: 'redirect' },
    res(421, err('misdirected')),
    res(200, report(ID, lost.opId, 'queue', TEXT, { kind: 'possible_duplicate_refused', earlier: earlierSend })),
    res(200, report(ID, lost.opId, 'queue', TEXT, { kind: 'conflicting_reuse', differs: ['text'] })),
  ];
  for (const refusal of refusals) {
    const retry = press(lost.state, QUEUE);
    assert.ok(retry);
    assert.equal(retry.opId, lost.opId, 'the retry reuses the op id');
    const after = settle(retry.state, retry.opId, sendAnswer(refusal, ID, retry.opId));
    assert.deepEqual(states(after), ['q1:unknown'], `still unknown after ${JSON.stringify(refusal).slice(0, 80)}`);
    assert.equal(after.draft, TEXT, 'the text stays');
  }
  // A first attempt refused before the core is just not sent: it leaves the list, and the next press is the same op id.
  const d = draft();
  const first = pressAndSettle(d, QUEUE, res(403, err('forbidden_origin')));
  assert.deepEqual(states(first.state), ['q1:refused'], 'shown, with its number unused');
  assert.equal(first.state.said.get('queue:1')?.kind, 'not-sent');
  assert.equal(press(first.state, QUEUE)?.opId, first.opId);
});

test('2: one attempt’s outcome never touches another: a failed or refused Send again leaves the earlier one uncertain', () => {
  const v = press(draft(), QUEUE);
  assert.ok(v);
  const uncertain = settle(v.state, v.opId, sendAnswer(res(200, report(ID, v.opId, 'queue', TEXT, outcome(DELIVERIES.uncertain))), ID, v.opId));
  for (const [a, second] of [
    [(op: string) => res(200, report(ID, op, 'queue', TEXT, outcome(failedArchived))), 'q2:done'],
    [(op: string) => res(200, report(ID, op, 'queue', TEXT, { kind: 'possible_duplicate_refused', earlier: earlierSend })), 'q2:refused'],
    [() => res(503, err('busy')), 'q2:refused'],
    [() => res(401), 'q2:refused'],
  ] as const) {
    const again = press(uncertain, DUPLICATE);
    assert.ok(again);
    const after = settle(again.state, again.opId, sendAnswer(a(again.opId), ID, again.opId));
    assert.deepEqual(states(after), ['q1:uncertain', second], 'the earlier attempt is untouched; the new one shows its own outcome');
    assert.equal(after.draft, TEXT);
    assert.equal(check(after, open(after))?.opId, v.opId, 'Check again still reads it');
  }
  // A lost Send again: both stay, and both survive a reload.
  const again = press(uncertain, DUPLICATE);
  assert.ok(again);
  const both = restore(again.state);
  assert.deepEqual(states(both), ['q1:uncertain', 'q2:unknown']);
  assert.equal(press(both, QUEUE)?.opId, again.opId, 'Try again is the second attempt, same op id');
  // A sent Send again settles only itself.
  const sentAgain = settle(again.state, again.opId, sendAnswer(res(200, report(ID, again.opId, 'queue', TEXT, outcome(DELIVERIES.sent))), ID, again.opId));
  assert.deepEqual(states(sentAgain), ['q1:uncertain', 'q2:done']);
  assert.equal(sentAgain.attempts[1]?.result, 'sent');
  assert.equal(sentAgain.draft, TEXT, 'not finished while the earlier attempt is unresolved');
});

test('a lookup that finds nothing, or fails, never settles an attempt; its own record does, and never allocates an id', () => {
  const lost = pressAndSettle(draft(), QUEUE, { kind: 'timeout' });
  for (const a of [res(200, lookup(ID, lost.opId, null)), res(401), { kind: 'network' } as Answer, res(500, err('record_unavailable'))]) {
    const c = check(lost.state, open(lost.state));
    assert.ok(c);
    assert.equal(c.opId, lost.opId);
    const after = checked(c.state, ID, c.opId, a);
    assert.deepEqual(states(after), ['q1:unknown']);
    assert.equal(after.draft, TEXT);
    assert.ok(after.checked);
  }
  const c = check(lost.state, open(lost.state));
  assert.ok(c);
  const u = checked(c.state, ID, c.opId, res(200, lookup(ID, lost.opId, report(ID, lost.opId, 'queue', TEXT, outcome(DELIVERIES.uncertain)))));
  assert.deepEqual([states(u), u.next], [['q1:uncertain'], { queue: 2, steer: 1 }]);
  const c2 = check(u, open(u));
  assert.ok(c2);
  const again = checked(c2.state, ID, c2.opId, res(200, lookup(ID, lost.opId, report(ID, lost.opId, 'queue', TEXT, outcome(DELIVERIES.uncertain)))));
  assert.deepEqual(again.next, { queue: 2, steer: 1 }, 'reading the same record again allocates nothing');
  const c3 = check(again, open(again));
  assert.ok(c3);
  const found = checked(c3.state, ID, c3.opId, res(200, lookup(ID, lost.opId, report(ID, lost.opId, 'queue', TEXT, outcome(DELIVERIES.queuedBusy)))));
  assert.deepEqual([states(found), found.attempts[0]?.result, found.draft, found.root], [['q1:done'], 'queued', '', null], 'a recorded queued report settles it, and the draft is done');
});

test('op ids are derived from the draft’s root, mode and attempt number; renewed only once used', () => {
  const root = newOpId();
  assert.match(root, /^web-[0-9a-f]{32}$/);
  assert.notEqual(newOpId(), root);
  assert.equal(opIdOf(root, 'queue', 1), `${root}-q1`);
  assert.equal(opIdOf(root, 'steer', 12), `${root}-s12`);
  const s = draft();
  assert.ok(s.root);
  assert.equal(edit(s, `${TEXT} edited`, nextRoot).root, s.root, 'editing keeps the root');
  assert.equal(edit(EMPTY, '  ', nextRoot).root, null, 'no root without text');
  const first = press(s, QUEUE);
  assert.ok(first);
  assert.equal(first.opId, `${s.root}-q1`);
  assert.equal(press(s, STEER)?.opId, `${s.root}-s1`);
  assert.equal(press(first.state, QUEUE), null, 'no second send while one is out');
  assert.deepEqual(first.body, { op_id: first.opId, mode: 'queue', text: TEXT, resume_paused_queue: false, accept_possible_duplicate: false });
  // A reload while sending: unknown, the same op id, and a retry reuses it.
  const reloaded = restore(first.state);
  assert.deepEqual(states(reloaded), ['q1:unknown']);
  assert.deepEqual(reloaded.said.get('queue:1'), { kind: 'unknown', cause: 'reload', status: null, error: null });
  assert.equal(press(reloaded, QUEUE)?.opId, first.opId);
  // Not sent: the next press, of any text, is the same op id (it was never claimed).
  const notSent = settle(first.state, first.opId, sendAnswer(res(503, err('busy')), ID, first.opId));
  assert.equal(press(edit(notSent, `${TEXT} edited`, nextRoot), QUEUE)?.opId, first.opId);
  // Queued or sent: the text and the root are gone.
  for (const d of [DELIVERIES.sent, DELIVERIES.queuedInteraction]) {
    const done = settle(first.state, first.opId, sendAnswer(res(200, report(ID, first.opId, 'queue', TEXT, outcome(d))), ID, first.opId));
    assert.deepEqual({ draft: done.draft, root: done.root, rows: states(done) }, { draft: '', root: null, rows: ['q1:done'] }, 'done; the row stays in memory');
  }
  // Failed or a conflicting reuse: the text stays, that mode's next attempt is n + 1. A possible duplicate keeps n (never claimed).
  for (const [r, n] of [
    [report(ID, first.opId, 'queue', TEXT, outcome(DELIVERIES.failed)), 2],
    [report(ID, first.opId, 'queue', TEXT, { kind: 'conflicting_reuse', differs: ['text'] }), 2],
    [report(ID, first.opId, 'queue', TEXT, { kind: 'possible_duplicate_refused', earlier: [] }), 1],
  ] as const) {
    const done = settle(first.state, first.opId, sendAnswer(res(200, r), ID, first.opId));
    assert.equal(done.attempts.some((x) => isUnsettled(x.state)), false);
    assert.equal(done.draft, TEXT);
    assert.deepEqual(done.next, { queue: n, steer: 1 }, r.status);
    assert.equal(press(done, QUEUE)?.opId, `${s.root}-q${n}`);
  }
  // Forgetting keeps the root and the numbers: nothing is ever sent again under a used op id.
  const u = settle(first.state, first.opId, sendAnswer(res(200, report(ID, first.opId, 'queue', TEXT, outcome(DELIVERIES.uncertain))), ID, first.opId));
  assert.equal(press(forget(u), QUEUE)?.opId, `${s.root}-q2`);
  assert.equal(press(draft('  \n '), QUEUE), null, 'nothing to send');
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

function store(storage = new MemoryStorage()) {
  const clock = new FakeClock();
  const f = new FakeFetch();
  const composers = new Composers({ fetch: f.fetch, storage, timers: clock, newOpId: nextRoot });
  return { clock, f, storage, composers };
}

const posts = (f: FakeFetch) => f.inits.filter((i) => i.method === 'POST').length;

test('the store: one POST per press, to the open thread, stored before it leaves', async () => {
  const h = store();
  h.composers.edit(ID, TEXT);
  void h.composers.press(ID, QUEUE);
  void h.composers.press(ID, QUEUE);
  void h.composers.press(ID, STEER);
  await flush();
  assert.equal(posts(h.f), 1, 'one request for three presses');
  assert.equal(h.f.urls[0], `/api/v1/sessions/${ID}/messages`);
  const init = h.f.inits[0];
  assert.ok(init);
  assert.deepEqual(init.headers, {
    Accept: 'application/json',
    'X-Requested-With': 'XMLHttpRequest',
    'Content-Type': 'application/json',
    'X-OpenWiz-Send': '1',
  });
  assert.equal(init.redirect, 'manual');
  assert.equal(init.credentials, 'same-origin');
  const body = JSON.parse(String(init.body));
  const stored = JSON.parse(h.storage.getItem(`openwiz.composer.${ID}`) ?? 'null');
  assert.equal(`${stored.root}-q${stored.attempts[0].n}`, body.op_id, 'the attempt is stored before the answer');
  assert.equal(stored.attempts[0].state, 'sending');
  h.f.reply(json(200, report(ID, body.op_id, 'queue', TEXT, outcome(DELIVERIES.sent))));
  await flush();
  assert.equal(h.composers.get(ID).said.get('queue:1')?.kind, 'report');
  assert.equal(h.storage.getItem(`openwiz.composer.${ID}`), null, 'the text leaves storage once sent');
  assert.equal(posts(h.f), 1);
});

test('the store: no answer in 45 s is uncertain; Check again only reads; a reload keeps the op id', async () => {
  const h = store();
  h.composers.edit(ID, TEXT);
  void h.composers.press(ID, QUEUE);
  await flush();
  const opId = JSON.parse(String(h.f.inits[0]?.body)).op_id;
  await h.clock.advance(SEND_TIMEOUT_MS - 1);
  assert.deepEqual(states(h.composers.get(ID)), ['q1:sending']);
  await h.clock.advance(1);
  assert.deepEqual(states(h.composers.get(ID)), ['q1:unknown']);
  assert.equal(h.composers.get(ID).said.get('queue:1')?.kind, 'unknown');
  void h.composers.check(ID, open(h.composers.get(ID)));
  await flush();
  assert.equal(h.f.urls.at(-1), `/api/v1/sessions/${ID}/messages/${opId}`);
  assert.equal(h.f.inits.at(-1)?.method, 'GET');
  h.f.reply(json(200, lookup(ID, opId, null)));
  await flush();
  assert.deepEqual(states(h.composers.get(ID)), ['q1:unknown']);
  assert.deepEqual(h.composers.get(ID).checked, { key: 'queue:1', note: { kind: 'no-record' } });
  // A reload: a new store over the same storage has the same attempt; Try again reuses its op id.
  const after = store(h.storage);
  assert.deepEqual(states(after.composers.get(ID)), ['q1:unknown']);
  void after.composers.press(ID, QUEUE);
  await flush();
  assert.equal(JSON.parse(String(after.f.inits[0]?.body)).op_id, opId);
  assert.equal(posts(h.f), 1);
});

test('the store: a thread keeps its own draft; corrupt storage is an empty composer', () => {
  const h = store();
  h.composers.edit(ID, 'one');
  h.composers.edit('thr_web_0002', 'two');
  assert.equal(h.composers.get(ID).draft, 'one');
  assert.equal(h.composers.get('thr_web_0002').draft, 'two');
  h.storage.setItem('openwiz.composer.thr_web_0003', '{"draft": 1}');
  h.storage.setItem('openwiz.composer.thr_web_0004', 'not json');
  assert.deepEqual(store(h.storage).composers.get('thr_web_0003'), EMPTY);
  assert.deepEqual(store(h.storage).composers.get('thr_web_0004'), EMPTY);
});

test('the preview is the core\'s, for this thread and mode, or why there is none', async () => {
  const h = store();
  const ctl = new AbortController();
  const p = h.composers.preview(ID, 'steer', ctl.signal);
  await flush();
  assert.equal(h.f.urls[0], `/api/v1/sessions/${ID}/preview?mode=steer`);
  h.f.reply(json(200, preview(ID, 'busy', 'steer')));
  const said = await p;
  assert.equal(said?.kind, 'ok');
  const q = h.composers.preview(ID, 'queue', ctl.signal);
  await flush();
  h.f.reply(json(200, preview(ID, 'busy', 'steer')));
  assert.equal((await q)?.kind, 'failed', 'a preview for another mode is not this one');
});



const stored = (s: StorageLike, id = ID) => JSON.parse(s.getItem(`openwiz.composer.${id}`) ?? 'null');

/** A storage whose writes start failing (a full quota) when `full` is set. */
class QuotaStorage extends MemoryStorage {
  full = false;
  override setItem(key: string, value: string) {
    if (this.full) throw new DOMException('quota', 'QuotaExceededError');
    super.setItem(key, value);
  }
}

test('the op ids belong to the draft: minted and stored on the first edit, before any press', () => {
  const h = store();
  h.composers.edit(ID, TEXT);
  const s = stored(h.storage);
  assert.match(s?.root ?? '', /^web-[0-9a-f]{32}$/);
  assert.deepEqual(s?.next, { queue: 1, steer: 1 });
  assert.equal(posts(h.f), 0);
});

test('a duplicated tab with an unsent draft sends with the same op id as the first', async () => {
  const a = store();
  a.composers.edit(ID, TEXT);
  const copy = new MemoryStorage();
  for (const [k, v] of a.storage.map) copy.setItem(k, v);
  void a.composers.press(ID, QUEUE);
  await flush();
  const first = JSON.parse(String(a.f.inits[0]?.body));
  a.f.reply(json(200, report(ID, first.op_id, 'queue', TEXT, outcome(DELIVERIES.sent))));
  await flush();
  const b = store(copy);
  void b.composers.press(ID, QUEUE);
  await flush();
  const second = JSON.parse(String(b.f.inits[0]?.body ?? 'null'));
  assert.equal(second?.op_id, first.op_id, 'the copy reuses the draft’s op id, so the core replays');
});

test('a lost answer, then a retry refused as a possible duplicate, stays uncertain with its op id', async () => {
  const h = store();
  h.composers.edit(ID, TEXT);
  void h.composers.press(ID, QUEUE);
  await flush();
  const opId = JSON.parse(String(h.f.inits[0]?.body)).op_id;
  h.f.fail();
  await flush();
  assert.deepEqual(states(h.composers.get(ID)), ['q1:unknown']);
  void h.composers.press(ID, QUEUE);
  await flush();
  assert.equal(JSON.parse(String(h.f.inits[1]?.body)).op_id, opId, 'the retry reuses the op id');
  h.f.reply(json(200, report(ID, opId, 'queue', TEXT, { kind: 'possible_duplicate_refused', earlier: earlierSend })));
  await flush();
  const s = h.composers.get(ID);
  assert.deepEqual(states(s), ['q1:unknown'], 'still uncertain: the refusal speaks for the retry only');
  assert.equal(s.draft, TEXT);
});

test('no POST unless the binding is stored; full or missing storage disables sending with a reason', async () => {
  const full = new QuotaStorage();
  const h = store(full);
  h.composers.edit(ID, TEXT);
  full.full = true;
  void h.composers.press(ID, QUEUE);
  await flush();
  assert.equal(posts(h.f), 0, 'nothing is sent when the op id can’t be stored');
  assert.match(h.composers.blocked(ID) ?? '', /can’t be saved/);
  const none = new Composers({ fetch: new FakeFetch().fetch, storage: null, timers: new FakeClock(), newOpId: nextRoot });
  assert.match(none.blocked(ID) ?? '', /can’t be saved/);
});

test('storage holds the draft, its root, the next numbers and the unresolved attempts only; never a report', async () => {
  const h = store();
  h.composers.edit(ID, TEXT);
  void h.composers.press(ID, QUEUE);
  await flush();
  assert.deepEqual(Object.keys(stored(h.storage)).sort(), ['attempts', 'draft', 'next', 'root']);
  const opId = JSON.parse(String(h.f.inits[0]?.body)).op_id;
  h.f.reply(json(200, report(ID, opId, 'queue', TEXT, outcome(DELIVERIES.sent))));
  await flush();
  assert.equal(h.composers.get(ID).said.get('queue:1')?.kind, 'report', 'the report is shown, from memory');
  assert.equal(h.storage.getItem(`openwiz.composer.${ID}`), null, 'nothing is left once sent');
  // Failed: the text stays, with fresh ids; still no report in storage.
  h.composers.edit(ID, TEXT);
  void h.composers.press(ID, QUEUE);
  await flush();
  const op2 = JSON.parse(String(h.f.inits[1]?.body)).op_id;
  h.f.reply(json(200, report(ID, op2, 'queue', TEXT, outcome(DELIVERIES.failed))));
  await flush();
  const raw = h.storage.getItem(`openwiz.composer.${ID}`) ?? '';
  assert.deepEqual(Object.keys(JSON.parse(raw)).sort(), ['attempts', 'draft', 'next', 'root']);
  for (const leak of ['summary', 'text_sha256', 'Synthetic web session', 'observ']) assert.equal(raw.includes(leak), false, `no ${leak} in storage`);
});



const bodyOf = (f: FakeFetch, i: number) => JSON.parse(String(f.inits[i]?.body ?? 'null'));
const copyOf = (s: MemoryStorage) => {
  const c = new MemoryStorage();
  for (const [k, v] of s.map) c.setItem(k, v);
  return c;
};
const uncertainFor = (opId: string) => json(200, report(ID, opId, 'queue', TEXT, outcome(DELIVERIES.uncertain)));

test('1a: a lookup that reads an uncertain record keeps the reserved successor; copied tabs resend with one id', async () => {
  const one = store();
  one.composers.edit(ID, TEXT);
  void one.composers.press(ID, QUEUE);
  await flush();
  const a = bodyOf(one.f, 0).op_id;
  one.f.reply(uncertainFor(a));
  await flush();
  const two = store(copyOf(one.storage));
  // Tab one checks A (still uncertain) while tab two sends again.
  void one.composers.check(ID, open(one.composers.get(ID)));
  await flush();
  one.f.reply(json(200, lookup(ID, a, report(ID, a, 'queue', TEXT, outcome(DELIVERIES.uncertain)))));
  await flush();
  void two.composers.press(ID, DUPLICATE);
  await flush();
  void one.composers.press(ID, DUPLICATE);
  await flush();
  const b2 = bodyOf(two.f, 0)?.op_id;
  const b1 = bodyOf(one.f, 2)?.op_id;
  assert.ok(b1 && b2, 'both tabs sent again');
  assert.notEqual(b1, a);
  assert.equal(b1, b2, 'one successor: the same op id in both tabs');
});

test('1b: copied tabs that each process the same uncertain report derive the same successor', async () => {
  const one = store();
  one.composers.edit(ID, TEXT);
  void one.composers.press(ID, QUEUE);
  await flush();
  const a = bodyOf(one.f, 0).op_id;
  const two = store(copyOf(one.storage)); // copied while A is out: it restores A as unknown
  one.f.reply(uncertainFor(a));
  await flush();
  void two.composers.press(ID, QUEUE); // Try again, same op id
  await flush();
  assert.equal(bodyOf(two.f, 0).op_id, a);
  two.f.reply(uncertainFor(a));
  await flush();
  void one.composers.press(ID, DUPLICATE);
  void two.composers.press(ID, DUPLICATE);
  await flush();
  assert.equal(bodyOf(one.f, 1)?.op_id, bodyOf(two.f, 1)?.op_id, 'the same successor in both tabs');
  assert.notEqual(bodyOf(one.f, 1)?.op_id, a);
});

test('2: a failed new attempt leaves the earlier uncertain attempt uncertain, with its lookup, through a reload', async () => {
  const h = store();
  h.composers.edit(ID, TEXT);
  void h.composers.press(ID, QUEUE);
  await flush();
  const a = bodyOf(h.f, 0).op_id;
  h.f.reply(uncertainFor(a));
  await flush();
  void h.composers.press(ID, DUPLICATE);
  await flush();
  const b = bodyOf(h.f, 1).op_id;
  assert.notEqual(b, a);
  h.f.reply(json(200, report(ID, b, 'queue', TEXT, outcome({ kind: 'failed', reason: 'target_archived', text: 'failed: the target is archived, and archived targets are refused' }))));
  await flush();
  // Check again still looks up A, here and after a reload.
  for (const s of [h, store(h.storage)]) {
    void s.composers.check(ID, open(s.composers.get(ID)));
    await flush();
    assert.equal(s.f.urls.at(-1), `/api/v1/sessions/${ID}/messages/${a}`, 'Check again reads the earlier attempt');
    s.f.reply(json(200, lookup(ID, a, report(ID, a, 'queue', TEXT, outcome(DELIVERIES.uncertain)))));
    await flush();
  }
  assert.equal(posts(h.f), 2, 'checking never sends');
});



test('2: Forget it retires every number used, even an attempt with no answer; copied tabs agree on the next', async () => {
  const one = store();
  one.composers.edit(ID, TEXT);
  void one.composers.press(ID, QUEUE);
  await flush();
  const a = bodyOf(one.f, 0).op_id;
  one.f.fail(); // the provider took it; the acknowledgement was lost
  await flush();
  const two = store(copyOf(one.storage));
  for (const tab of [one, two]) {
    tab.composers.forget(ID);
    void tab.composers.press(ID, QUEUE);
    await flush();
  }
  const b1 = bodyOf(one.f, 1)?.op_id;
  const b2 = bodyOf(two.f, 0)?.op_id;
  assert.ok(b1 && b2);
  assert.notEqual(b1, a, 'never the forgotten op id');
  assert.equal(b1, b2, 'both tabs send the same next attempt');
  assert.equal(b1, a.replace(/-q1$/, '-q2'));
});
