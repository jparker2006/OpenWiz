// The page's state: the last good snapshot is kept through failures, the
// selection survives refreshes (V5), states are honest (V6), and the list and
// detail each have at most one read in flight (V4).

import assert from 'node:assert/strict';
import { test } from 'node:test';

import { connection, isStale, listed, Viewer } from '../../src/viewer.ts';
import { TIMING } from '../../src/timing.ts';
import { inventory, session, sessionView } from '../support/synthetic.ts';
import { FakeClock, FakeFetch, FakeVisibility, json, settle } from './support.ts';

/** The wall clock starts here and moves with the fake clock. */
const WALL0 = 1_799_913_600_000;

function harness() {
  const clock = new FakeClock();
  const f = new FakeFetch();
  const visibility = new FakeVisibility();
  const viewer = new Viewer({ fetch: f.fetch, timers: clock, visibility, now: () => clock.now, wall: () => WALL0 + clock.now }, TIMING);
  let changes = 0;
  viewer.subscribe(() => changes++);
  return { clock, f, visibility, viewer, changes: () => changes };
}

const at = (ms: number) => ({ mono: ms, wall: WALL0 + ms });

const page = inventory([session(1), session(2), session(3)]);

test('the first list read starts at once and its envelope becomes the snapshot', async () => {
  const h = harness();
  assert.equal(connection(h.viewer.getState().list, at(0), TIMING.staleAfterMs), 'reading');
  h.viewer.start();
  await settle();
  assert.deepEqual(h.f.urls, ['/api/v1/inventory']);
  await h.clock.advance(1_234);
  h.f.reply(json(200, page));
  await settle();
  const list = h.viewer.getState().list;
  assert.deepEqual(list.last, { envelope: page, receivedAt: 1_234, receivedWall: WALL0 + 1_234 });
  assert.equal(list.failure, null);
  assert.equal(list.offset, 0);
  assert.ok(h.changes() > 0);
  h.viewer.stop();
});

test('a failure keeps the last good snapshot, and the next success clears it (V6)', async () => {
  const h = harness();
  h.viewer.start();
  await settle();
  h.f.reply(json(200, page));
  await h.clock.advance(TIMING.listIntervalMs);
  h.f.fail();
  await settle();
  let list = h.viewer.getState().list;
  assert.deepEqual(list.last?.envelope, page);
  assert.deepEqual(list.failure, { kind: 'disconnected', cause: 'network', status: null });
  assert.equal(connection(list, at(h.clock.now), TIMING.staleAfterMs), 'disconnected');
  // A network failure is not the server's answer: the retry waits out its bound since the read was sent.
  await h.clock.advance(TIMING.laneHoldMs);
  h.f.reply(json(200, page));
  await settle();
  list = h.viewer.getState().list;
  assert.equal(list.failure, null);
  assert.equal(connection(list, at(h.clock.now), TIMING.staleAfterMs), 'live');
  h.viewer.stop();
});

test('stale is measured on the device\'s clocks since the response arrived, not by the snapshot time (Q1)', () => {
  const snap = { receivedAt: 1_000, receivedWall: 1_000_000 };
  assert.equal(isStale(snap, { mono: 31_000, wall: 1_030_000 }, 30_000), false);
  assert.equal(isStale(snap, { mono: 31_001, wall: 1_030_001 }, 30_000), true);
  // Asleep: the monotonic clock stood still while the wall clock moved on. Stale.
  assert.equal(isStale(snap, { mono: 1_500, wall: 1_000_000 + 3_600_000 }, 30_000), true);
  // The wall clock set back: the monotonic clock still counts. Stale.
  assert.equal(isStale(snap, { mono: 40_000, wall: 0 }, 30_000), true);
  const list = { offset: 0, last: { envelope: inventory([], { snapshotAt: 0 }), receivedAt: 5_000, receivedWall: 5_000 }, failure: null, previousMatched: null };
  // A snapshot time far in the past does not make it stale; the arrival time does.
  assert.equal(connection(list, { mono: 6_000, wall: 6_000 }, 30_000), 'live');
  assert.equal(connection(list, { mono: 36_000, wall: 36_000 }, 30_000), 'stale');
});

test('each failure has its own connection word', () => {
  const base = { offset: 0, last: null, previousMatched: null };
  assert.equal(connection({ ...base, failure: { kind: 'signed-out', status: 401 } }, at(0), 30_000), 'signed-out');
  assert.equal(connection({ ...base, failure: { kind: 'disconnected', cause: 'gateway', status: 530 } }, at(0), 30_000), 'disconnected');
  assert.equal(connection({ ...base, failure: { kind: 'busy', status: 503, retryAfterMs: 5_000, error: null } }, at(0), 30_000), 'busy');
  assert.equal(
    connection({ ...base, failure: { kind: 'error', status: 500, error: { kind: 'untested_version', message: 'x' } } }, at(0), 30_000),
    'error',
  );
});

test('the selection survives refreshes; when its row leaves the page it is kept and marked (V5)', async () => {
  const h = harness();
  h.viewer.start();
  await settle();
  h.f.reply(json(200, page));
  await settle();
  h.viewer.select('thr_web_0002');
  await settle();
  assert.deepEqual(h.f.urls, ['/api/v1/inventory', '/api/v1/sessions/thr_web_0002']);
  h.f.reply(json(200, sessionView(session(2))));
  await settle();
  assert.equal(h.viewer.getState().detail?.id, 'thr_web_0002');
  assert.equal(listed(h.viewer.getState()), true);
  // The next list read no longer has it.
  await h.clock.advance(TIMING.listIntervalMs);
  h.f.reply(json(200, inventory([session(1), session(3)])));
  await settle();
  const state = h.viewer.getState();
  assert.equal(state.detail?.id, 'thr_web_0002', 'still open');
  assert.deepEqual(state.detail?.last?.envelope, sessionView(session(2)), 'its details are kept');
  assert.equal(listed(state), false);
  h.viewer.stop();
});

test('the list and the detail each have at most one read in flight, also across a change of session (V4)', async () => {
  const h = harness();
  h.viewer.start();
  await settle();
  h.f.reply(json(200, page));
  await settle();
  h.viewer.select('thr_web_0001');
  await h.clock.advance(10_000); // a list read and the detail read are both in flight
  const detailUrls = () => h.f.urls.filter((u) => u.startsWith('/api/v1/sessions/'));
  assert.deepEqual(detailUrls(), ['/api/v1/sessions/thr_web_0001']);
  h.viewer.select('thr_web_0003');
  await settle();
  assert.deepEqual(detailUrls(), ['/api/v1/sessions/thr_web_0001'], 'the new session waits for the read in flight');
  assert.equal(h.f.open().some((o) => o.url === '/api/v1/sessions/thr_web_0001'), true, 'which is not aborted: the server would keep running it');
  assert.equal(h.viewer.getState().detail?.id, 'thr_web_0003');
  h.f.replyTo('/api/v1/sessions/thr_web_0001', json(200, sessionView(session(1))));
  await settle();
  assert.deepEqual(detailUrls(), ['/api/v1/sessions/thr_web_0001', '/api/v1/sessions/thr_web_0003']);
  assert.equal(h.viewer.getState().detail?.last, null, 'the old session\'s answer is not shown as the new one');
  await h.clock.advance(10 * 60_000);
  assert.equal(h.f.maxInFlight.list, 1);
  assert.equal(h.f.maxInFlight.detail, 1);
  h.viewer.stop();
  assert.deepEqual(h.f.inFlight, { list: 0, detail: 0 });
});

test('closing the detail starts no more detail reads', async () => {
  const h = harness();
  h.viewer.start();
  await settle();
  h.f.reply(json(200, page));
  await settle();
  h.viewer.select('thr_web_0001');
  await settle();
  h.viewer.select(null);
  assert.equal(h.viewer.getState().detail, null);
  const detailReads = () => h.f.urls.filter((u) => u.startsWith('/api/v1/sessions/')).length;
  const before = detailReads();
  await h.clock.advance(5 * 60_000);
  assert.equal(detailReads(), before);
  assert.equal(h.f.inFlight.detail, 0, 'the read in flight ended (here, at its timeout)');
  h.viewer.stop();
});

test('another page waits for the list read in flight, then reads its offset; the open detail stays, marked (V4, V8)', async () => {
  const h = harness();
  h.viewer.start();
  await settle();
  h.f.reply(json(200, inventory([session(1)], { matched: 250 })));
  await settle();
  h.viewer.select('thr_web_0001');
  await settle();
  h.f.replyTo('/api/v1/sessions/thr_web_0001', json(200, sessionView(session(1))));
  await h.clock.advance(TIMING.listIntervalMs); // a list read is in flight again
  const listUrls = () => h.f.urls.filter((u) => u.startsWith('/api/v1/inventory'));
  assert.equal(listUrls().length, 2);
  h.viewer.goTo(100);
  await settle();
  assert.equal(listUrls().length, 2, 'the new page waits for the read in flight');
  assert.equal(h.f.open().some((o) => o.url === '/api/v1/inventory'), true, 'which is not aborted');
  let state = h.viewer.getState();
  assert.equal(state.list.offset, 100);
  assert.equal(state.list.last, null);
  assert.equal(state.detail?.id, 'thr_web_0001', 'the detail stays open');
  h.f.replyTo('/api/v1/inventory', json(200, inventory([session(1)], { matched: 250 })));
  await settle();
  assert.equal(h.viewer.getState().list.last, null, 'the old page\'s answer is not shown');
  assert.equal(listUrls().at(-1), '/api/v1/inventory?offset=100');
  h.f.replyTo('/api/v1/inventory?offset=100', json(200, inventory([session(101)], { offset: 100, matched: 250 })));
  await settle();
  state = h.viewer.getState();
  assert.equal(state.list.last?.envelope.data.sessions[0]?.id, 'thr_web_0101');
  assert.equal(listed(state), false, 'marked: no longer listed on this page');
  assert.equal(h.f.maxInFlight.list, 1);
  h.viewer.stop();
});

test('at the request timeout the page says the read is late, while the read goes on; its answer still counts', async () => {
  const h = harness();
  h.viewer.start();
  await settle();
  h.f.reply(json(200, page));
  await h.clock.advance(TIMING.listIntervalMs); // the next list read hangs
  await h.clock.advance(TIMING.requestTimeoutMs);
  let list = h.viewer.getState().list;
  assert.deepEqual(list.failure, { kind: 'disconnected', cause: 'timeout', status: null });
  assert.deepEqual(list.last?.envelope, page, 'the last good snapshot stays');
  assert.equal(h.f.inFlight.list, 1, 'the read is not abandoned');
  h.f.reply(json(200, inventory([session(9)])));
  await settle();
  list = h.viewer.getState().list;
  assert.equal(list.failure, null);
  assert.equal(list.last?.envelope.data.sessions[0]?.id, 'thr_web_0009');
  h.viewer.stop();
});

test('another page keeps the count of the page it came from, so a change between the two can be shown', async () => {
  const h = harness();
  h.viewer.start();
  await settle();
  h.f.reply(json(200, inventory([session(1)], { matched: 250 })));
  await settle();
  assert.equal(h.viewer.getState().list.previousMatched, null);
  h.viewer.goTo(100);
  await settle();
  assert.equal(h.viewer.getState().list.previousMatched, 250);
  h.f.replyTo('/api/v1/inventory?offset=100', json(200, inventory([session(102)], { offset: 100, matched: 249 })));
  await settle();
  const list = h.viewer.getState().list;
  assert.equal(list.previousMatched, 250);
  assert.equal(list.last?.envelope.coverage.counts?.matched, 249);
  h.viewer.stop();
});
