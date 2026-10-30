// V4 and V7 at the loop level: one read in flight, the next only after the
// last ended or timed out, capped backoff, paused while hidden.

import assert from 'node:assert/strict';
import { test } from 'node:test';

import { Lane, Poller } from '../../src/poller.ts';
import { FakeClock, FakeVisibility, settle } from './support.ts';

/** `confirmed: false`: the server is not known to be done with the read (no answer from it). */
type Result = { ok: boolean; retryAfterMs?: number | null; confirmed?: boolean };

/** A read the test finishes by hand; it counts reads in flight. */
function harness(o: { intervalMs?: number; timeoutMs?: number; holdMs?: number; backoffMs?: number[]; lane?: Lane; clock?: FakeClock } = {}) {
  const clock = o.clock ?? new FakeClock();
  const visibility = new FakeVisibility();
  const started: number[] = [];
  const delivered: Result[] = [];
  const timeouts: number[] = [];
  const open: { finish: (r: Result | 'stopped') => void; signal: AbortSignal }[] = [];
  let inFlight = 0;
  let maxInFlight = 0;
  const poller = new Poller<Result>({
    ...(o.lane ? { lane: o.lane } : {}),
    intervalMs: o.intervalMs ?? 10_000,
    timeoutMs: o.timeoutMs ?? 45_000,
    holdMs: o.holdMs ?? 70_000,
    backoffMs: o.backoffMs ?? [2_000, 4_000, 8_000, 16_000, 30_000],
    timers: clock,
    visibility,
    onTimeout: () => timeouts.push(clock.now),
    read: (signal) =>
      new Promise((resolve) => {
        started.push(clock.now);
        inFlight++;
        maxInFlight = Math.max(maxInFlight, inFlight);
        let done = false;
        const finish = (r: Result | 'stopped') => {
          if (done) return;
          done = true;
          inFlight--;
          resolve(r);
        };
        signal.addEventListener('abort', () => finish(signal.reason === 'timeout' ? { ok: false, confirmed: false } : 'stopped'));
        open.push({ finish, signal });
      }),
    settle: (r) => ({ ok: r.ok, retryAfterMs: r.retryAfterMs ?? null, confirmed: r.confirmed ?? true }),
    deliver: (r) => delivered.push(r),
  });
  const last = () => {
    const entry = open.at(-1);
    assert.ok(entry, 'a read was started');
    return entry;
  };
  return { clock, visibility, poller, started, delivered, timeouts, open, last, max: () => maxInFlight, inFlight: () => inFlight };
}

test('the first read starts at once, the next one interval after it ended', async () => {
  const h = harness();
  h.poller.start();
  await settle();
  assert.deepEqual(h.started, [0]);
  await h.clock.advance(3_000);
  h.last().finish({ ok: true });
  await h.clock.advance(9_999);
  assert.equal(h.started.length, 1, 'not before the interval');
  await h.clock.advance(1);
  assert.deepEqual(h.started, [0, 13_000]);
  assert.equal(h.delivered.length, 1);
});

test('a slow read delays the next refresh instead of overlapping it (V4)', async () => {
  const h = harness();
  h.poller.start();
  await settle();
  await h.clock.advance(40_000); // four intervals, still in flight
  assert.equal(h.started.length, 1);
  assert.equal(h.max(), 1);
  assert.equal(h.poller.inFlight, true);
  h.last().finish({ ok: true });
  await h.clock.advance(10_000);
  assert.deepEqual(h.started, [0, 50_000]);
  assert.equal(h.max(), 1);
});

test('a read that never answers: the page is told at its timeout, the read keeps its lane until the server\'s bound, then the retry follows the backoff (V4)', async () => {
  const h = harness({ timeoutMs: 45_000, holdMs: 70_000 });
  h.poller.start();
  await settle();
  await h.clock.advance(45_000);
  assert.deepEqual(h.timeouts, [45_000], 'the page is told when the read is late');
  assert.equal(h.last().signal.aborted, false, 'but the read is not abandoned: the server may still be working on it');
  assert.equal(h.inFlight(), 1);
  await h.clock.advance(24_999);
  assert.equal(h.started.length, 1, 'nothing new before the server\'s bound');
  await h.clock.advance(1);
  assert.equal(h.last().signal.aborted, true);
  assert.equal(h.last().signal.reason, 'timeout');
  assert.deepEqual(h.delivered, [{ ok: false, confirmed: false }]);
  await h.clock.advance(2_000);
  assert.deepEqual(h.started, [0, 72_000]);
  assert.equal(h.max(), 1);
});

test('an answer after the page\'s timeout still ends the read, is delivered, and frees the lane', async () => {
  const h = harness({ timeoutMs: 45_000, holdMs: 70_000 });
  h.poller.start();
  await settle();
  await h.clock.advance(50_000);
  h.last().finish({ ok: true });
  await settle();
  assert.deepEqual(h.delivered, [{ ok: true }]);
  await h.clock.advance(10_000);
  assert.deepEqual(h.started, [0, 60_000]);
});

test('a failure the server did not confirm holds the lane until the bound since the read was sent', async () => {
  const h = harness({ holdMs: 70_000 });
  h.poller.start();
  await settle();
  await h.clock.advance(1_000);
  h.last().finish({ ok: false, confirmed: false }); // a dropped connection: the server may still be working
  await h.clock.advance(68_999);
  assert.equal(h.started.length, 1, 'not after the 2 s backoff: the lane is held');
  await h.clock.advance(1);
  assert.deepEqual(h.started, [0, 70_000]);
});

test('a failure the server confirmed frees the lane at once: the backoff alone applies', async () => {
  const h = harness({ holdMs: 70_000 });
  h.poller.start();
  await settle();
  await h.clock.advance(1_000);
  h.last().finish({ ok: false, confirmed: true }); // the server's own error answer
  await h.clock.advance(2_000);
  assert.deepEqual(h.started, [0, 3_000]);
});

test('a replacing loop waits for the held lane of the loop it replaced (V4)', async () => {
  const clock = new FakeClock();
  const lane = new Lane();
  const a = harness({ clock, lane, holdMs: 70_000 });
  a.poller.start();
  await settle();
  await clock.advance(1_000);
  a.last().finish({ ok: false, confirmed: false });
  a.poller.retire();
  const b = harness({ clock, lane, holdMs: 70_000 });
  b.poller.start();
  await clock.advance(68_000);
  assert.equal(b.started.length, 0, 'the new page waits while the old read may still run on the server');
  await clock.advance(1_000);
  assert.deepEqual(b.started, [70_000]);
});

test('failures back off up to the cap, and a success resets it (V7)', async () => {
  const h = harness({ backoffMs: [2_000, 4_000, 8_000, 16_000, 30_000] });
  h.poller.start();
  await settle();
  for (let i = 0; i < 8; i++) {
    h.last().finish({ ok: false });
    // Up to the next read's start only, so no read reaches its own timeout.
    const reads = h.started.length;
    for (let t = 0; t < 60 && h.started.length === reads; t++) await h.clock.advance(1_000);
  }
  const gaps = h.started.slice(1).map((t, i) => t - (h.started[i] ?? 0));
  assert.deepEqual(gaps.slice(0, 8), [2_000, 4_000, 8_000, 16_000, 30_000, 30_000, 30_000, 30_000]);
  h.last().finish({ ok: true });
  const before = h.started.length;
  await h.clock.advance(10_000);
  assert.equal(h.started.length, before + 1, 'back to the interval after a success');
});

test('Retry-After is honoured as the shortest wait', async () => {
  const h = harness({ backoffMs: [2_000, 4_000, 8_000] });
  h.poller.start();
  await settle();
  h.last().finish({ ok: false, retryAfterMs: 5_000 });
  await h.clock.advance(4_999);
  assert.equal(h.started.length, 1);
  await h.clock.advance(1);
  assert.equal(h.started.length, 2);
});

test('a server\'s retry hint never waits past the backoff cap', async () => {
  const h = harness({ backoffMs: [2_000, 4_000, 8_000, 16_000, 30_000] });
  h.poller.start();
  await settle();
  h.last().finish({ ok: false, retryAfterMs: 999_999_000 }); // Retry-After: 999999
  await h.clock.advance(30_000);
  assert.equal(h.started.length, 2, 'retried at the 30 s cap, not in 11.6 days');
});

test('refresh pauses while hidden, and runs once when the page returns (V7)', async () => {
  const h = harness();
  h.poller.start();
  await settle();
  h.last().finish({ ok: true });
  h.visibility.set(true);
  await h.clock.advance(10 * 60_000);
  assert.equal(h.started.length, 1, 'no read while hidden');
  assert.equal(h.clock.pending(), 0, 'no timer left running while hidden');
  h.visibility.set(false);
  await settle();
  assert.equal(h.started.length, 2, 'one read on return');
  await h.clock.advance(5_000);
  assert.equal(h.started.length, 2, 'only one');
  h.last().finish({ ok: true });
  await h.clock.advance(10_000);
  assert.equal(h.started.length, 3, 'then the interval again');
});

test('returning while a read is in flight does not start a second one', async () => {
  const h = harness();
  h.poller.start();
  await settle();
  h.visibility.set(true);
  h.visibility.set(false);
  await settle();
  assert.equal(h.started.length, 1);
  assert.equal(h.max(), 1);
});

test('hidden at start: nothing is read until the page is visible', async () => {
  const h = harness();
  h.visibility.set(true);
  h.poller.start();
  await h.clock.advance(60_000);
  assert.equal(h.started.length, 0);
  h.visibility.set(false);
  await settle();
  assert.equal(h.started.length, 1);
});

test('stop aborts the read in flight and starts nothing more', async () => {
  const h = harness();
  h.poller.start();
  await settle();
  h.poller.stop();
  assert.equal(h.last().signal.aborted, true);
  assert.equal(h.last().signal.reason, 'stop');
  await h.clock.advance(10 * 60_000);
  h.visibility.set(true);
  h.visibility.set(false);
  await settle();
  assert.equal(h.started.length, 1);
  assert.equal(h.delivered.length, 0, 'a stopped read delivers nothing');
  assert.equal(h.clock.pending(), 0);
});
