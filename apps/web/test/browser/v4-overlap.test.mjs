// V4: reads never overlap. With a server slower than the interval, no second
// list or detail request starts until the first ends or times out. The page
// runs on Playwright's fake clock; the fake API counts requests in flight.

import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';

import { TIMING } from '../../src/timing.ts';
import { DESKTOP, launchBrowser, openPage, readDetail, readView, settleReal, waitFor } from '../support/browser.mjs';
import { DROP_HELD, HANG, json } from '../support/fake-api.mjs';
import { fakeSite } from '../support/harness.mjs';
import { inventory, manyRows, session, sessionView } from '../support/synthetic.ts';

const START = new Date('2027-01-15T08:00:00Z');

/** How many intervals fit before the read's own timeout: slower than the interval, not yet timed out. */
const under = (interval) => Math.floor((TIMING.requestTimeoutMs - 1) / interval);

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(() => browser.close());

async function setup() {
  const fx = await fakeSite();
  fx.api.setList(() => json(200, inventory(manyRows(5))));
  fx.api.setDetail((id) => json(200, sessionView(session(Number(id.slice(-4))))));
  return fx;
}

test('a list read slower than the interval is never overlapped', async () => {
  const fx = await setup();
  fx.api.hold('list');
  const p = await openPage(browser, fx.site.origin, { clock: START });
  try {
    await waitFor(() => fx.api.reads('list').length === 1, { what: 'the first list read' });
    assert.ok(under(TIMING.listIntervalMs) >= 2, 'the read is held for at least two intervals');
    for (let i = 0; i < under(TIMING.listIntervalMs); i++) {
      await p.page.clock.runFor(TIMING.listIntervalMs);
      await settleReal(30);
    }
    assert.equal(fx.api.reads('list').length, 1, 'no second list read while the first is in flight');
    assert.equal(fx.api.maxInFlight.list, 1);
    fx.api.hold('list', false);
    fx.api.release('list');
    await waitFor(async () => (await readView(p.page)).groups.length > 0, { what: 'rows' });
    await p.page.clock.runFor(TIMING.listIntervalMs);
    await waitFor(() => fx.api.reads('list').length === 2, { what: 'the next list read, one interval later' });
    assert.equal(fx.api.maxInFlight.list, 1);
  } finally {
    await p.close();
    await fx.close();
  }
});

test('a detail read slower than the interval is never overlapped, and the list keeps its own single read', async () => {
  const fx = await setup();
  const p = await openPage(browser, fx.site.origin, { clock: START });
  try {
    await waitFor(async () => (await readView(p.page)).groups.length > 0, { what: 'rows' });
    fx.api.hold('detail');
    await p.page.click('[data-session-id="thr_web_0002"]');
    await waitFor(() => fx.api.reads('detail').length === 1, { what: 'the detail read' });
    assert.ok(under(TIMING.detailIntervalMs) >= 2, 'the read is held for at least two intervals');
    for (let i = 0; i < under(TIMING.detailIntervalMs); i++) {
      await p.page.clock.runFor(TIMING.detailIntervalMs);
      await settleReal(30);
    }
    assert.equal(fx.api.reads('detail').length, 1, 'no second detail read while the first is in flight');
    assert.equal(fx.api.maxInFlight.detail, 1);
    assert.ok(fx.api.reads('list').length >= 3, 'the list kept refreshing');
    assert.equal(fx.api.maxInFlight.list, 1);
    fx.api.hold('detail', false);
    fx.api.release('detail');
    await waitFor(async () => (await readDetail(p.page))?.title === 'Synthetic web session 2', { what: 'the detail' });
    await p.page.clock.runFor(TIMING.detailIntervalMs);
    await waitFor(() => fx.api.reads('detail').length === 2, { what: 'the next detail read' });
    assert.equal(fx.api.maxInFlight.detail, 1);
  } finally {
    await p.close();
    await fx.close();
  }
});

test('a read that never answers: at its timeout the page says so, the request stays open until the server\'s bound, then the retry', async () => {
  const fx = await setup();
  fx.api.setList(() => HANG);
  const p = await openPage(browser, fx.site.origin, { clock: START });
  try {
    await waitFor(() => fx.api.reads('list').length === 1, { what: 'the first list read' });
    await p.page.clock.runFor(TIMING.requestTimeoutMs + 1_000);
    await waitFor(async () => (await readView(p.page)).connection === 'disconnected', { what: 'the late read shown' });
    await settleReal(100);
    assert.equal(fx.api.reads('list')[0].endedAt, null, 'the request is still open: the server may still be working on it');
    await p.page.clock.runFor(TIMING.laneHoldMs - TIMING.requestTimeoutMs - 2_000);
    await settleReal(100);
    assert.equal(fx.api.reads('list').length, 1);
    assert.equal(fx.api.reads('list')[0].endedAt, null);
    await p.page.clock.runFor(1_001);
    await waitFor(() => fx.api.reads('list')[0].endedAt !== null, { what: 'the read given up at the server\'s bound' });
    await p.page.clock.runFor(TIMING.backoffMs[0]);
    await waitFor(() => fx.api.reads('list').length === 2, { what: 'the retry after backoff' });
    assert.equal(fx.api.maxInFlight.list, 1);
  } finally {
    await p.close();
    await fx.close();
  }
});

/** A fake whose list is 250 rows, paged by offset. */
function paged(fx) {
  const all = manyRows(250);
  fx.api.setList((url) => {
    const offset = Number(url.searchParams.get('offset') ?? 0);
    return json(200, inventory(all.slice(offset, offset + 100), { offset, limit: 100, matched: all.length }));
  });
}

test('a page change after the page\'s timeout still waits for the server to answer the old read', async () => {
  const fx = await setup();
  paged(fx);
  const p = await openPage(browser, fx.site.origin, { clock: START, viewport: DESKTOP });
  try {
    await waitFor(async () => (await readView(p.page)).range === '1–100 of 250', { what: 'page 1' });
    fx.api.hold('list');
    await p.page.clock.runFor(TIMING.listIntervalMs);
    await waitFor(() => fx.api.held('list') === 1, { what: 'a held refresh' });
    await p.page.clock.runFor(TIMING.requestTimeoutMs + 1_000);
    await waitFor(async () => (await readView(p.page)).connection === 'disconnected', { what: 'the late read shown' });
    await p.page.click('[data-part=next]');
    for (let i = 0; i < 4; i++) {
      await p.page.clock.runFor(4_000);
      await settleReal(50);
    }
    assert.equal(fx.api.reads('list').length, 2, 'page 2 waits: the server is still running page 1\'s read');
    fx.api.hold('list', false);
    fx.api.release('list'); // the server answers within its bound
    await waitFor(async () => (await readView(p.page)).range === '101–200 of 250', { what: 'page 2, after the server answered' });
    assert.equal(fx.api.maxInFlight.list, 1);
  } finally {
    await p.close();
    await fx.close();
  }
});

test('after a dropped connection, a page change waits for the server\'s bound since the read was sent', async () => {
  const fx = await setup();
  const all = manyRows(250);
  let drop = false;
  fx.api.setList((url) => {
    if (drop) {
      drop = false;
      return DROP_HELD;
    }
    const offset = Number(url.searchParams.get('offset') ?? 0);
    return json(200, inventory(all.slice(offset, offset + 100), { offset, limit: 100, matched: all.length }));
  });
  const p = await openPage(browser, fx.site.origin, { clock: START, viewport: DESKTOP });
  try {
    await waitFor(async () => (await readView(p.page)).range === '1–100 of 250', { what: 'page 1' });
    drop = true;
    await p.page.clock.runFor(TIMING.listIntervalMs);
    await waitFor(async () => (await readView(p.page)).connection === 'disconnected', { what: 'the dropped read' });
    await p.page.click('[data-part=next]');
    for (let i = 0; i < 8; i++) {
      await p.page.clock.runFor(8_000);
      await settleReal(50);
    }
    assert.equal(fx.api.reads('list').length, 2, 'page 2 waits out the server\'s bound (64 s of 70 so far)');
    fx.api.release('list'); // the server finishes the dropped read within its bound
    await p.page.clock.runFor(TIMING.laneHoldMs - 64_000);
    await waitFor(async () => (await readView(p.page)).range === '101–200 of 250', { what: 'page 2, after the bound' });
    assert.equal(fx.api.maxInFlight.list, 1);
  } finally {
    await p.close();
    await fx.close();
  }
});

test('changing page while a list read is held starts no second list read until it ends', async () => {
  const fx = await setup();
  const all = manyRows(250);
  fx.api.setList((url) => {
    const offset = Number(url.searchParams.get('offset') ?? 0);
    return json(200, inventory(all.slice(offset, offset + 100), { offset, limit: 100, matched: all.length }));
  });
  const p = await openPage(browser, fx.site.origin, { clock: START, viewport: DESKTOP });
  try {
    await waitFor(async () => (await readView(p.page)).range === '1–100 of 250', { what: 'page 1' });
    fx.api.hold('list');
    await p.page.clock.runFor(TIMING.listIntervalMs);
    await waitFor(() => fx.api.held('list') === 1, { what: 'a held refresh' });
    await p.page.click('[data-part=next]');
    for (let i = 0; i < 3; i++) {
      await p.page.clock.runFor(5_000);
      await settleReal(50);
    }
    assert.equal(fx.api.reads('list').length, 2, 'page 2 is not read while page 1\'s read is still running');
    assert.equal(fx.api.maxInFlight.list, 1);
    fx.api.hold('list', false);
    fx.api.release('list');
    await waitFor(async () => (await readView(p.page)).range === '101–200 of 250', { what: 'page 2, after the held read ended' });
    assert.equal(fx.api.reads('list').at(-1).search, '?offset=100');
    assert.equal(fx.api.maxInFlight.list, 1);
  } finally {
    await p.close();
    await fx.close();
  }
});

test('choosing another session while a detail read is held starts no second detail read until it ends', async () => {
  const fx = await setup();
  const p = await openPage(browser, fx.site.origin, { clock: START, viewport: DESKTOP });
  try {
    await waitFor(async () => (await readView(p.page)).groups.length > 0, { what: 'rows' });
    fx.api.hold('detail');
    await p.page.click('[data-session-id="thr_web_0002"]');
    await waitFor(() => fx.api.held('detail') === 1, { what: 'a held detail read' });
    await p.page.click('[data-session-id="thr_web_0003"]');
    for (let i = 0; i < 3; i++) {
      await p.page.clock.runFor(5_000);
      await settleReal(50);
    }
    assert.equal(fx.api.reads('detail').length, 1, 'the second session is not read while the first read is still running');
    assert.equal(fx.api.maxInFlight.detail, 1);
    fx.api.hold('detail', false);
    fx.api.release('detail');
    await waitFor(async () => (await readDetail(p.page))?.title === 'Synthetic web session 3', { what: 'the second session' });
    assert.equal(fx.api.maxInFlight.detail, 1);
  } finally {
    await p.close();
    await fx.close();
  }
});
