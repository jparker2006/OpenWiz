// V11, synthetic half: measurements on fixtures. There are no performance
// targets yet (no baseline on a real account), so these record numbers; the
// only assertions are fixed ones: no layout shift on refresh, and at most one
// list and one detail read in flight.

import assert from 'node:assert/strict';
import { mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { after, before, test } from 'node:test';
import { brotliCompressSync, constants, gzipSync } from 'node:zlib';

import { TIMING } from '../../src/timing.ts';
import { DESKTOP, launchBrowser, openPage, PHONE, readDetail, readView, waitFor } from '../support/browser.mjs';
import { json } from '../support/fake-api.mjs';
import { fakeSite } from '../support/harness.mjs';
import { OUT_DIR, RESULTS_DIR } from '../support/paths.mjs';
import { inventory, manyRows, session, sessionView } from '../support/synthetic.ts';

const START = new Date('2027-01-15T08:00:00Z');
const results = {};

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(async () => {
  await browser.close();
  mkdirSync(RESULTS_DIR, { recursive: true });
  writeFileSync(`${RESULTS_DIR}/v11-perf.json`, `${JSON.stringify(results, null, 2)}\n`);
});

const pct = (xs, q) => {
  const s = xs.toSorted((a, b) => a - b);
  return s[Math.min(s.length - 1, Math.floor(q * s.length))];
};

async function siteWith(rows, delayMs = 0) {
  const fx = await fakeSite();
  const body = json(200, inventory(manyRows(rows), { matched: rows }));
  fx.api.setList(() => ({ ...body, delayMs }));
  fx.api.setDetail((id) => ({ ...json(200, sessionView(session(Number(id.slice(-4))))), delayMs }));
  return { fx, body };
}

test('P5: JavaScript, CSS and font payload, compressed', async () => {
  const { fx } = await siteWith(5);
  const p = await openPage(browser, fx.site.origin);
  try {
    await waitFor(async () => (await readView(p.page)).groups.length > 0, { what: 'rows' });
    const loaded = [...new Set(p.requests.map((r) => new URL(r.url).pathname).filter((x) => x !== '/' && !x.startsWith('/api/')))];
    const rows = loaded.map((path) => {
      const file = path === '/' ? `${OUT_DIR}/index.html` : `${OUT_DIR}${path}`;
      const raw = readFileSync(file);
      return {
        path,
        bytes: statSync(file).size,
        gzip: gzipSync(raw, { level: 9 }).length,
        brotli: brotliCompressSync(raw, { params: { [constants.BROTLI_PARAM_QUALITY]: 11 } }).length,
      };
    });
    const html = readFileSync(`${OUT_DIR}/index.html`);
    rows.push({ path: '/ (index.html)', bytes: html.length, gzip: gzipSync(html, { level: 9 }).length, brotli: brotliCompressSync(html).length });
    const sum = (kind, key) => rows.filter((r) => kind.test(r.path)).reduce((a, r) => a + r[key], 0);
    results.payload = {
      files: rows,
      js: { bytes: sum(/\.js$/, 'bytes'), gzip: sum(/\.js$/, 'gzip'), brotli: sum(/\.js$/, 'brotli') },
      css: { bytes: sum(/\.css$/, 'bytes'), gzip: sum(/\.css$/, 'gzip'), brotli: sum(/\.css$/, 'brotli') },
      fonts: { bytes: sum(/\.woff2$/, 'bytes') },
    };
  } finally {
    await p.close();
    await fx.close();
  }
});

test('P1: time to first rows, cold, local and throttled (synthetic)', async () => {
  results.firstRows = {};
  for (const [label, rows, net] of [
    ['local, 6 rows', 6, null],
    ['local, 100 rows', 100, null],
    ['throttled (150 ms RTT, 1.6 Mbit/s), 100 rows', 100, { latency: 150, downloadThroughput: 200_000, uploadThroughput: 90_000 }],
  ]) {
    const times = [];
    for (let i = 0; i < 10; i++) {
      const { fx } = await siteWith(rows);
      const context = await browser.newContext({ viewport: PHONE, timezoneId: 'UTC' });
      try {
        const page = await context.newPage();
        if (net) {
          const cdp = await context.newCDPSession(page);
          await cdp.send('Network.emulateNetworkConditions', { offline: false, ...net });
        }
        await page.goto(fx.site.origin);
        await page.waitForFunction(() => performance.getEntriesByName('first-rows').length > 0, null, { timeout: 30_000 });
        times.push(await page.evaluate(() => performance.getEntriesByName('first-rows')[0].startTime));
      } finally {
        await context.close();
        await fx.close();
      }
    }
    results.firstRows[label] = { runs: times.length, p50: pct(times, 0.5), p95: pct(times, 0.95), min: Math.min(...times), max: Math.max(...times) };
  }
});

test('P3: tap to the sheet opening, and to its details', async () => {
  const { fx } = await siteWith(30);
  const p = await openPage(browser, fx.site.origin, { viewport: PHONE });
  try {
    await waitFor(async () => (await readView(p.page)).groups.length > 0, { what: 'rows' });
    const open = [];
    const loaded = [];
    for (let i = 1; i <= 20; i++) {
      const id = `thr_web_${String(i).padStart(4, '0')}`;
      await p.page.evaluate(() => performance.clearMarks());
      await p.page.click(`[data-session-id="${id}"]`);
      await waitFor(async () => (await readDetail(p.page))?.title === `Synthetic web session ${i}`, { what: id });
      const m = await p.page.evaluate(() => {
        const get = (n) => performance.getEntriesByName(n).at(-1)?.startTime ?? null;
        return { tap: get('detail-tap'), shown: get('detail-shown'), loaded: get('detail-loaded') };
      });
      open.push(m.shown - m.tap);
      loaded.push(m.loaded - m.tap);
      await p.page.keyboard.press('Escape');
      await waitFor(async () => (await readDetail(p.page)) === null, { what: 'close' });
    }
    results.tapToDetail = {
      opened: { p50: pct(open, 0.5), p95: pct(open, 0.95) },
      loaded: { p50: pct(loaded, 0.5), p95: pct(loaded, 0.95) },
    };
  } finally {
    await p.close();
    await fx.close();
  }
});

test('P4, P6, P7, P8: bytes, main-thread work and layout shift per refresh', async () => {
  results.refresh = {};
  for (const rows of [6, 100, 200]) {
    const { fx, body } = await siteWith(rows);
    let n = 0;
    // Every refresh changes the statuses, so each one renders.
    fx.api.setList(() => {
      n++;
      return json(
        200,
        inventory(
          manyRows(rows).map((s, i) => ({ ...s, status: { known: ['idle', 'active', 'error'][(i + n) % 3] } })),
          { matched: rows },
        ),
      );
    });
    const p = await openPage(browser, fx.site.origin, { viewport: DESKTOP, clock: START });
    const cdp = await p.context.newCDPSession(p.page);
    try {
      await waitFor(async () => (await readView(p.page)).groups.length > 0, { what: 'rows' });
      await p.page.click('[data-session-id="thr_web_0001"]');
      await waitFor(async () => (await readDetail(p.page))?.title === 'Synthetic web session 1', { what: 'detail' });
      await p.page.evaluate(() => {
        window.__shifts = 0;
        new PerformanceObserver((list) => {
          for (const e of list.getEntries()) if (!e.hadRecentInput) window.__shifts += e.value;
        }).observe({ type: 'layout-shift' });
      });
      await cdp.send('Performance.enable');
      const metric = async () => Object.fromEntries((await cdp.send('Performance.getMetrics')).metrics.map((m) => [m.name, m.value]));
      const before = await metric();
      const refreshes = 20;
      for (let i = 0; i < refreshes; i++) {
        const k = fx.api.reads('list').length;
        await p.page.clock.runFor(TIMING.listIntervalMs);
        await waitFor(() => fx.api.reads('list').length > k && fx.api.reads('list').at(-1).endedAt !== null, { what: 'refresh' });
        await waitFor(async () => (await p.page.evaluate(() => document.querySelector('[data-snapshot-seq]')?.dataset.snapshotSeq)) === String(k + 1), {
          what: 'render',
        });
      }
      const after = await metric();
      const per = (key) => ((after[key] - before[key]) * 1000) / refreshes;
      const shifts = await p.page.evaluate(() => window.__shifts);
      results.refresh[`${rows} rows`] = {
        bytesPerRefresh: Buffer.byteLength(body.body),
        mainThreadMsPerRefresh: { task: per('TaskDuration'), script: per('ScriptDuration'), layout: per('LayoutDuration'), style: per('RecalcStyleDuration') },
        layoutShift: shifts,
        maxInFlight: fx.api.maxInFlight,
      };
      assert.equal(shifts, 0, `layout shift on refresh with ${rows} rows`);
      assert.ok(fx.api.maxInFlight.list <= 1 && fx.api.maxInFlight.detail <= 1);
    } finally {
      await p.close();
      await fx.close();
    }
  }
});
