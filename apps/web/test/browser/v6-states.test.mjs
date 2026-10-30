// V6: states are honest. Stale, partial, disconnected, signed out, busy and
// each core error have their own word and shape, with the last good snapshot
// time. Stale is measured on the device's clock; a sign-in redirect is never a
// network error. Cloudflare's and Access's responses here are synthetic.

import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';

import { TIMING } from '../../src/timing.ts';
import { launchBrowser, openPage, PHONE, readDetail, readView, waitFor } from '../support/browser.mjs';
import { apiError, bare, DROP, HANG, html, json } from '../support/fake-api.mjs';
import { fakeSite } from '../support/harness.mjs';
import { decodePng, inkMask, maskDifference } from '../support/png.mjs';
import { inventory, manyRows, session, sessionView, T0 } from '../support/synthetic.ts';

const START = new Date(T0 + 5 * 60_000);
const GOOD = inventory(manyRows(3), { snapshotAt: T0 });
const LAST_GOOD = '08:00:00';

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(() => browser.close());

/**
 * Serves one good snapshot, then `failure` for every later list read (or for
 * every read when `first` is false). Returns the page in that state.
 */
async function inState(failure, { first = true, detail = null } = {}) {
  const fx = await fakeSite();
  let n = 0;
  fx.api.setList(() => (first && n++ === 0 ? json(200, GOOD) : failure));
  if (detail) fx.api.setDetail(detail);
  const p = await openPage(browser, fx.site.origin, { clock: START, viewport: PHONE });
  if (first) await waitFor(async () => (await readView(p.page)).groups.length > 0, { what: 'the good snapshot' });
  return { fx, p };
}

async function nextFailure(p, fx, ms = TIMING.listIntervalMs) {
  const reads = fx.api.reads('list').length;
  await p.page.clock.runFor(ms);
  await waitFor(() => fx.api.reads('list').length > reads, { what: 'the next read' });
  await waitFor(async () => (await readView(p.page)).connection !== 'live', { what: 'the state to change' });
}

test('stale: the last good snapshot is older than the threshold on the device clock', async () => {
  const { fx, p } = await inState(HANG);
  try {
    assert.equal((await readView(p.page)).connection, 'live');
    await p.page.clock.runFor(TIMING.staleAfterMs - 1_000);
    let view = await readView(p.page);
    assert.equal(view.connection, 'live', 'not stale before the threshold');
    assert.equal(view.stale, false);
    await p.page.clock.runFor(2_000);
    view = await waitFor(async () => {
      const v = await readView(p.page);
      return v.connection === 'stale' ? v : null;
    });
    assert.equal(view.stale, true);
    assert.ok(view.snapshot.includes(LAST_GOOD), 'the last good snapshot time is shown');
    assert.equal(view.groups.length > 0, true, 'the old rows stay, marked stale');
  } finally {
    await p.close();
    await fx.close();
  }
});

test('partial: the envelope\'s coverage and its gaps, word for word', async () => {
  const gaps = [
    { kind: 'command_failed', subject: 'thr_web_0002', detail: 'thread show exited 1: Error: HTTP 500: Synthetic server error' },
    { kind: 'timed_out', subject: null, detail: 'machine list timed out after 15 s' },
  ];
  const fx = await fakeSite();
  fx.api.setList(() => json(200, inventory(manyRows(3), { state: 'partial', gaps })));
  const p = await openPage(browser, fx.site.origin, { clock: START });
  try {
    await waitFor(async () => (await readView(p.page)).groups.length > 0, { what: 'rows' });
    let view = await readView(p.page);
    assert.equal(view.coverage, 'partial · 2 gaps');
    assert.equal(view.connection, 'live', 'partial is not a connection state');
    assert.deepEqual(view.gaps, [], 'the reasons are one tap away');
    await p.page.click('[data-part=coverage]');
    view = await readView(p.page);
    assert.deepEqual(view.gaps, [
      'command_failed · thr_web_0002 · thread show exited 1: Error: HTTP 500: Synthetic server error',
      'timed_out · machine list timed out after 15 s',
    ]);
  } finally {
    await p.close();
    await fx.close();
  }
});

const failures = [
  ['disconnected', 'no answer at all (the connection drops)', DROP],
  ['disconnected', 'a Cloudflare tunnel error page (synthetic, 530)', html(530, '<!doctype html><title>Synthetic tunnel error</title>error code: 1033')],
  ['disconnected', 'a gateway error page (synthetic, 502)', html(502, '<html>Synthetic bad gateway</html>')],
  ['signed-out', 'Access\'s 401 to a sub-request (synthetic)', html(401, '<html>Synthetic: sign in again</html>')],
  ['signed-out', 'a redirect to the sign-in page (synthetic)', bare(302, { Location: 'https://signin.example.invalid/cdn-cgi/access/login' })],
  ['busy', '503 busy with Retry-After', apiError(503, 'busy', 'too many requests at once; retry shortly', { 'Retry-After': '5' })],
  ['busy', 'a bodyless 503', bare(503, { 'Retry-After': '5' })],
  ['error', 'read_failed', apiError(502, 'read_failed', 'thread list exited 1: Error: HTTP 500: Synthetic server error')],
  ['error', 'untested_version', apiError(500, 'untested_version', 'room-cli 0.44.0 is not the tested version 0.43.1')],
  ['error', 'invalid_request', apiError(400, 'invalid_request', '--offset is not a number')],
  ['error', 'an unexpected answer', html(200, '<html>Synthetic captive portal</html>')],
  ['error', 'a 403 block page (synthetic): not signed out, since signing in would not fix it', html(403, '<html>Synthetic: access denied</html>')],
  ['disconnected', 'a 503 page from the network (synthetic), not the server being busy', html(503, '<html>Synthetic: service unavailable</html>')],
];

for (const [word, what, failure] of failures) {
  test(`${word}: ${what}, with the last good snapshot time`, async () => {
    const { fx, p } = await inState(failure);
    try {
      await nextFailure(p, fx);
      const view = await waitFor(async () => {
        const v = await readView(p.page);
        return v.connection === word ? v : null;
      }, { what: word });
      assert.ok(view.notice, 'a notice says what happened');
      assert.equal(view.notice.kind, word);
      assert.ok(view.notice.text.includes(LAST_GOOD), `the notice gives the last good snapshot time: ${view.notice.text}`);
      if (typeof failure === 'object' && failure.body?.startsWith('{')) {
        const { error } = JSON.parse(failure.body);
        assert.ok(view.notice.text.includes(error.kind) && view.notice.text.includes(error.message), 'the server\'s own words');
      }
      assert.ok(view.groups.length > 0, 'the last good rows stay');
      assert.notEqual(view.connection, 'live');
      if (word === 'signed-out') {
        const href = await p.page.$eval('[data-notice=signed-out] a', (a) => a.getAttribute('href'));
        assert.equal(href, '/', 'signing in again is a page load, which Access redirects');
      }
    } finally {
      await p.close();
      await fx.close();
    }
  });
}

test('a failure before any snapshot says so, and never shows "No sessions"', async () => {
  const { fx, p } = await inState(html(530, 'error code: 1033'), { first: false });
  try {
    const view = await waitFor(async () => {
      const v = await readView(p.page);
      return v.connection === 'disconnected' ? v : null;
    });
    assert.equal(view.empty, null);
    assert.ok(view.notice.text.includes('no snapshot yet'));
  } finally {
    await p.close();
    await fx.close();
  }
});

test('"No sessions" only after a complete, successful, fresh read', async () => {
  const fx = await fakeSite();
  let n = 0;
  fx.api.setList(() => (n++ === 0 ? json(200, inventory([])) : HANG));
  const p = await openPage(browser, fx.site.origin, { clock: START });
  try {
    await waitFor(async () => (await readView(p.page)).empty === 'verified-empty', { what: 'verified empty' });
    await p.page.clock.runFor(TIMING.staleAfterMs + 1_000);
    await waitFor(async () => (await readView(p.page)).empty === 'empty-stale', { what: 'no longer claimed fresh' });
  } finally {
    await p.close();
    await fx.close();
  }
  const partial = await fakeSite();
  partial.api.setList(() => json(200, inventory([], { state: 'partial', gaps: [{ kind: 'timed_out', subject: null, detail: 'thread list timed out' }] })));
  const q = await openPage(browser, partial.site.origin, { clock: START });
  try {
    await waitFor(async () => (await readView(q.page)).connection === 'live', { what: 'the partial read' });
    assert.equal((await readView(q.page)).empty, 'empty-partial');
  } finally {
    await q.close();
    await partial.close();
  }
});

test('a detail the core no longer finds shows the core\'s words in the sheet', async () => {
  const { fx, p } = await inState(json(200, GOOD), { detail: (id) => apiError(404, 'not_found', `session ${id} not found`) });
  try {
    await p.page.click('[data-session-id="thr_web_0002"]');
    const d = await waitFor(async () => (await readDetail(p.page))?.notice ?? null, { what: 'the detail notice' });
    assert.equal(d.kind, 'error');
    assert.ok(d.text.includes('not_found') && d.text.includes('session thr_web_0002 not found'));
  } finally {
    await p.close();
    await fx.close();
  }
});

test('each connection state has its own word and its own shape, also in greyscale', async () => {
  const shapes = {};
  const states = [
    ['live', null],
    ['stale', HANG],
    ['disconnected', html(530, 'error code: 1033')],
    ['signed-out', html(401, 'Synthetic')],
    ['busy', bare(503, { 'Retry-After': '5' })],
    ['error', apiError(502, 'read_failed', 'Synthetic')],
  ];
  for (const [word, failure] of states) {
    const { fx, p } = await inState(failure ?? json(200, GOOD));
    try {
      if (word === 'stale') await p.page.clock.runFor(TIMING.staleAfterMs + 1_000);
      else if (failure) await nextFailure(p, fx);
      await waitFor(async () => (await readView(p.page)).connection === word, { what: word });
      await p.page.evaluate(() => (document.documentElement.style.filter = 'grayscale(1)'));
      const pill = p.page.locator('[data-connection] .shape');
      const box = await pill.boundingBox();
      const png = await p.page.screenshot({ clip: { x: box.x - 4, y: box.y - 4, width: box.width + 8, height: box.height + 8 } });
      shapes[word] = { text: (await readView(p.page)).connectionText, mask: inkMask(decodePng(png)) };
    } finally {
      await p.close();
      await fx.close();
    }
  }
  const words = Object.keys(shapes);
  assert.equal(new Set(words.map((w) => shapes[w].text)).size, words.length, 'each has its own word');
  for (let i = 0; i < words.length; i++) {
    for (let j = i + 1; j < words.length; j++) {
      const d = maskDifference(shapes[words[i]].mask, shapes[words[j]].mask);
      assert.ok(d >= 6, `${words[i]} and ${words[j]} have the same shape in greyscale (${d} px differ)`);
    }
  }
});

test('stale data is never shown as live: a hidden page that returns is stale until its read lands', async () => {
  const fx = await fakeSite();
  let n = 0;
  fx.api.setList(() => (n++ === 0 ? json(200, GOOD) : HANG));
  const p = await openPage(browser, fx.site.origin, { clock: START });
  try {
    await waitFor(async () => (await readView(p.page)).connection === 'live', { what: 'live' });
    await p.page.evaluate(() => {
      Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => 'hidden' });
      Object.defineProperty(document, 'hidden', { configurable: true, get: () => true });
      document.dispatchEvent(new Event('visibilitychange'));
    });
    await p.page.clock.runFor(10 * 60_000);
    await p.page.evaluate(() => {
      Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => 'visible' });
      Object.defineProperty(document, 'hidden', { configurable: true, get: () => false });
      document.dispatchEvent(new Event('visibilitychange'));
    });
    const view = await waitFor(async () => {
      const v = await readView(p.page);
      return v.connection !== 'live' ? v : null;
    });
    assert.equal(view.connection, 'stale');
  } finally {
    await p.close();
    await fx.close();
  }
});

test('the detail\'s coverage is its envelope\'s own state; its gaps are shown apart from it', async () => {
  const gap = { kind: 'timed_out', subject: 'thr_web_0002', detail: 'thread output timed out after 15 s' };
  const cases = [
    ['partial, no gaps', { state: 'partial', gaps: [] }, 'partial', []],
    ['complete, with a gap', { state: 'complete', gaps: [gap] }, 'complete', ['timed_out · thr_web_0002 · thread output timed out after 15 s']],
    ['complete, no gaps', { state: 'complete', gaps: [] }, 'complete', []],
  ];
  for (const [what, opts, word, gaps] of cases) {
    const fx = await fakeSite();
    fx.api.setList(() => json(200, GOOD));
    fx.api.setDetail(() => json(200, sessionView(session(2), opts)));
    const p = await openPage(browser, fx.site.origin, { clock: START });
    try {
      await waitFor(async () => (await readView(p.page)).groups.length > 0, { what: 'rows' });
      await p.page.click('[data-session-id="thr_web_0002"]');
      const d = await waitFor(async () => {
        const x = await readDetail(p.page);
        return x?.title ? x : null;
      });
      assert.equal(d.coverage, word, what);
      assert.deepEqual(d.gaps, gaps, what);
    } finally {
      await p.close();
      await fx.close();
    }
  }
});
