// V7: bounded over time. A recorded soak run on the fake clock: two simulated
// hours of refreshes with changing rows, then failures (backoff is capped),
// then a hidden hour (no reads; one on return). The JS heap must not grow.

import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync } from 'node:fs';
import { after, before, test } from 'node:test';

import { TIMING } from '../../src/timing.ts';
import { launchBrowser, openPage, readDetail, readView, settleReal, waitFor } from '../support/browser.mjs';
import { apiError, bare, html, json } from '../support/fake-api.mjs';
import { fakeSite } from '../support/harness.mjs';
import { RESULTS_DIR } from '../support/paths.mjs';
import { inventory, manyRows, session, sessionView } from '../support/synthetic.ts';

const START = new Date('2027-01-15T08:00:00Z');
const HOURS = 2;

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(() => browser.close());

async function heap(cdp) {
  await cdp.send('HeapProfiler.collectGarbage');
  await cdp.send('HeapProfiler.collectGarbage');
  return (await cdp.send('Runtime.getHeapUsage')).usedSize;
}

const setHidden = (page, hidden) =>
  page.evaluate((h) => {
    Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => (h ? 'hidden' : 'visible') });
    Object.defineProperty(document, 'hidden', { configurable: true, get: () => h });
    document.dispatchEvent(new Event('visibilitychange'));
  }, hidden);

test(`soak: ${HOURS} simulated hours, then failures, then a hidden hour`, { timeout: 900_000 }, async () => {
  const fx = await fakeSite();
  let n = 0;
  let failing = null;
  // Rows change on every read: a rotating window of 100 out of 400.
  fx.api.setList(() => {
    // The server's own error (it is done with the read), or the network's page (not known).
    if (failing === 'server') return apiError(502, 'read_failed', 'thread list exited 1: Error: HTTP 500: Synthetic server error');
    if (failing === 'network') return html(530, 'error code: 1033');
    const first = 1 + ((n++ * 7) % 300);
    return json(200, inventory(manyRows(100, first), { matched: 400 }));
  });
  fx.api.setDetail((id) => json(200, sessionView(session(Number(id.slice(-4))))));
  const p = await openPage(browser, fx.site.origin, { clock: START });
  const cdp = await p.context.newCDPSession(p.page);
  const record = { timing: TIMING, hours: HOURS, samples: [] };
  try {
    await waitFor(async () => (await readView(p.page)).groups.length > 0, { what: 'rows' });
    const id = await p.page.$eval('[data-session-id]', (el) => el.dataset.sessionId);
    await p.page.click(`[data-session-id="${id}"]`);
    await waitFor(async () => (await readDetail(p.page))?.title !== null, { what: 'the detail' });

    // Phase 1: healthy refreshes, the detail open.
    const steps = (HOURS * 3_600_000) / TIMING.listIntervalMs;
    let baseline = 0;
    for (let i = 1; i <= steps; i++) {
      const before = fx.api.reads('list').length;
      await p.page.clock.runFor(TIMING.listIntervalMs);
      await waitFor(() => fx.api.reads('list').length > before && fx.api.reads('list').at(-1).endedAt !== null, { what: `refresh ${i}` });
      if (i === 60) baseline = await heap(cdp);
      if (i % 120 === 0) record.samples.push({ refresh: i, usedSize: await heap(cdp) });
    }
    const end = await heap(cdp);
    record.baseline = baseline;
    record.end = end;
    assert.equal(fx.api.maxInFlight.list, 1);
    assert.equal(fx.api.maxInFlight.detail, 1);
    assert.ok(end <= baseline * 1.2 + 512 * 1024, `the heap grew from ${baseline} to ${end} bytes`);

    // Phase 2: 30 simulated minutes of failures the server confirms; the backoff is capped.
    failing = 'server';
    const failStart = fx.api.reads('list').length;
    const stamps = [];
    for (let t = 0; t < 30 * 60_000; t += 1_000) {
      const before = fx.api.reads('list').length;
      await p.page.clock.runFor(1_000);
      await settleReal(5);
      if (fx.api.reads('list').length > before) stamps.push(t + 1_000);
    }
    const gaps = stamps.slice(1).map((t, i) => t - (stamps[i] ?? 0));
    const cap = TIMING.backoffMs.at(-1);
    record.backoffGaps = gaps;
    assert.ok(gaps.length > 30, 'failures kept being retried');
    assert.ok(gaps.every((g) => g <= cap + 2_000), `a backoff above the cap: ${gaps}`);
    assert.ok(gaps.slice(-10).every((g) => Math.abs(g - cap) <= 2_000), 'the backoff reaches its cap and stays there');
    assert.ok(fx.api.reads('list').length - failStart <= (30 * 60_000) / cap + 10);

    // Phase 2b: 20 simulated minutes of the network's error page. The server is not
    // known to be done with those reads, so each holds the lane for its bound.
    failing = 'network';
    const netStamps = [];
    for (let t = 0; t < 20 * 60_000; t += 1_000) {
      const before = fx.api.reads('list').length;
      await p.page.clock.runFor(1_000);
      await settleReal(5);
      if (fx.api.reads('list').length > before) netStamps.push(t + 1_000);
    }
    const netGaps = netStamps.slice(1).map((t, i) => t - (netStamps[i] ?? 0));
    record.networkGaps = netGaps;
    assert.ok(netGaps.length >= 10, 'retried');
    assert.ok(netGaps.every((g) => g >= TIMING.laneHoldMs && g <= TIMING.laneHoldMs + 2_000), `spaced by the server's bound: ${netGaps}`);

    // Phase 3: hidden for an hour: no reads. One read on return.
    failing = null;
    await p.page.clock.runFor(TIMING.laneHoldMs + cap);
    await settleReal(100);
    await setHidden(p.page, true);
    await settleReal(100);
    const hiddenStart = { list: fx.api.reads('list').length, detail: fx.api.reads('detail').length };
    for (let t = 0; t < 60; t++) {
      await p.page.clock.runFor(60_000);
      await settleReal(5);
    }
    // A read already in flight when the page was hidden may finish; none starts.
    assert.equal(fx.api.reads('list').length, hiddenStart.list, 'no list read starts while hidden');
    assert.equal(fx.api.reads('detail').length, hiddenStart.detail, 'no detail read starts while hidden');
    const beforeReturn = fx.api.reads('list').length;
    await setHidden(p.page, false);
    await waitFor(() => fx.api.reads('list').length === beforeReturn + 1, { what: 'one read on return' });
    await settleReal(300);
    assert.equal(fx.api.reads('list').length, beforeReturn + 1, 'exactly one');
    record.result = 'pass';
  } finally {
    mkdirSync(RESULTS_DIR, { recursive: true });
    writeFileSync(`${RESULTS_DIR}/v7-soak.json`, `${JSON.stringify(record, null, 2)}\n`);
    await p.close();
    await fx.close();
  }
});

test('a server retry hint far in the future is capped at the backoff cap', async () => {
  const fx = await fakeSite();
  let n = 0;
  fx.api.setList(() => (n++ === 0 ? json(200, inventory(manyRows(3))) : n === 2 ? bare(503, { 'Retry-After': '999999' }) : json(200, inventory(manyRows(3)))));
  const p = await openPage(browser, fx.site.origin, { clock: START });
  try {
    await waitFor(async () => (await readView(p.page)).groups.length > 0, { what: 'rows' });
    await p.page.clock.runFor(TIMING.listIntervalMs);
    await waitFor(async () => (await readView(p.page)).connection === 'busy', { what: 'busy' });
    await p.page.clock.runFor((TIMING.backoffMs.at(-1) ?? 0) + 1_000);
    await waitFor(() => fx.api.reads('list').length === 3, { what: 'a retry within the cap, not 11.6 days later' });
  } finally {
    await p.close();
    await fx.close();
  }
});
