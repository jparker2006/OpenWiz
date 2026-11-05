// The browser for the proofs: the image's Chromium, driven by playwright-core
// (approved: W-5). Every page records its requests, console errors and
// Content Security Policy violations.

import { chromium } from 'playwright-core';

export const PHONE = { width: 390, height: 844 };
export const DESKTOP = { width: 1440, height: 900 };

export function launchBrowser() {
  return chromium.launch({ executablePath: process.env.OPENWIZ_CHROMIUM ?? '/usr/bin/chromium' });
}

/** A test-only slowdown: `OPENWIZ_CPU_THROTTLE=N` runs every page's CPU N times slower (DevTools' throttling). */
const CPU_THROTTLE = Number(process.env.OPENWIZ_CPU_THROTTLE ?? 1) || 1;

/**
 * Opens `origin` in a fresh context. `clock`: install Playwright's fake clock
 * at that time before the page loads; with `paused`, it never moves on its
 * own, only by `runFor` (otherwise it also flows in real time). `stampReads`:
 * record, in `window.__reads`, every API request the page makes and the
 * page's own clock (`Date.now()`) at the moment it called `fetch`, so a proof
 * can measure the page's timing exactly instead of when the server happened
 * to see a request; and count in `window.__readsDone` the requests whose
 * answer the page has read (or that failed), so a proof on a paused clock can
 * wait until the page has handled every answer before it moves time
 * (`pageIdle`).
 */
export async function openPage(browser, origin, { viewport = PHONE, clock = null, paused = false, stampReads = false, path = '/', hasTouch = false } = {}) {
  const context = await browser.newContext({
    viewport,
    deviceScaleFactor: 1,
    reducedMotion: 'reduce',
    serviceWorkers: 'block',
    hasTouch,
    timezoneId: 'UTC',
    locale: 'en-GB',
  });
  const page = await context.newPage();
  const requests = [];
  const responses = [];
  const errors = [];
  page.on('request', (r) => requests.push({ url: r.url(), method: r.method(), headers: r.headers() }));
  page.on('response', async (r) => {
    if (!new URL(r.url()).pathname.startsWith('/api/')) return;
    const entry = { path: new URL(r.url()).pathname + new URL(r.url()).search, status: r.status(), body: null };
    responses.push(entry);
    entry.body = await r.text().catch(() => null);
  });
  page.on('console', (m) => {
    // Chromium logs every non-2xx response as an error; the proofs make many on purpose.
    if (m.type() === 'error' && !m.text().startsWith('Failed to load resource')) errors.push(m.text());
  });
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(() => {
    window.__csp = [];
    document.addEventListener('securitypolicyviolation', (e) => window.__csp.push(`${e.violatedDirective} ${e.blockedURI}`));
  });
  if (stampReads) {
    await page.addInitScript(() => {
      window.__reads = [];
      window.__readsDone = 0;
      const real = window.fetch;
      window.fetch = (url, init) => {
        const path = String(url);
        if (!path.startsWith('/api/')) return real(url, init);
        window.__reads.push({ path, at: Date.now() });
        const done = () => {
          window.__readsDone++;
        };
        return real(url, init).then(
          (res) => {
            const text = res.text.bind(res);
            res.text = () => text().finally(done);
            return res;
          },
          (e) => {
            done();
            throw e;
          },
        );
      };
    });
  }
  if (clock !== null) {
    await page.clock.install({ time: clock });
    if (paused) await page.clock.pauseAt(clock);
  }
  if (CPU_THROTTLE > 1) await (await context.newCDPSession(page)).send('Emulation.setCPUThrottlingRate', { rate: CPU_THROTTLE });
  await page.goto(origin + path);
  return {
    context,
    page,
    requests,
    responses,
    errors,
    csp: () => page.evaluate(() => window.__csp),
    close: () => context.close(),
  };
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/**
 * With `stampReads`: whether the page has read the answer to every API request
 * it made. Its next timers are set by then (they follow in microtasks), so a
 * paused clock can be moved without racing the page.
 */
export const pageIdle = (page) => page.evaluate(() => window.__reads.length === window.__readsDone);

/** Polls `fn` from Node until it returns something truthy (works under a fake page clock). */
export async function waitFor(fn, { timeout = 15_000, interval = 25, what = 'condition' } = {}) {
  const end = Date.now() + timeout;
  let last;
  for (;;) {
    try {
      last = await fn();
      if (last) return last;
    } catch (e) {
      last = e;
    }
    if (Date.now() > end) throw new Error(`timed out waiting for ${what} (last: ${last instanceof Error ? last.message : JSON.stringify(last)})`);
    await sleep(interval);
  }
}

export const settleReal = (ms = 150) => sleep(ms);

/** The page as the proofs read it (the DOM contract in docs/dom.md terms). */
export function readView(page) {
  return page.evaluate(() => {
    const text = (el) => (el ? el.textContent : null);
    const q = (sel, root = document) => root.querySelector(sel);
    const marker = (root) => {
      const m = q('[data-part=status]', root);
      return m ? { word: text(q('.word', m)), glyph: text(q('.glyph', m)), tier: m.dataset.tier } : null;
    };
    const notice = q('main [data-notice]');
    return {
      connection: q('[data-connection]')?.dataset.connection ?? null,
      connectionText: text(q('[data-connection]')),
      coverage: text(q('[data-part=coverage]')),
      snapshot: text(q('[data-part=snapshot]')),
      stale: !!q('[data-part=stale]'),
      gaps: [...document.querySelectorAll('[data-part=gaps] li')].map((li) => li.textContent),
      notice: notice ? { kind: notice.dataset.notice, text: notice.textContent } : null,
      groups: [...document.querySelectorAll('[data-group-key]')].map((section) => ({
        key: section.dataset.groupKey,
        heading: text(q('h2', section)),
        rows: [...section.querySelectorAll('[data-session-id]')].map((row) => ({
          id: row.dataset.sessionId,
          title: text(q('[data-part=title]', row)),
          status: marker(row),
          age: text(q('[data-part=age]', row)),
          reason: text(q('[data-part=reason]', row)),
        })),
      })),
      empty: q('[data-part=empty]')?.dataset.kind ?? null,
      range: text(q('[data-part=range]')),
    };
  });
}

/** The open detail (sheet or panel), or null. */
export function readDetail(page) {
  return page.evaluate(() => {
    const d = document.querySelector('[data-part=detail]');
    if (!d || !d.open) return null;
    const field = (name) => d.querySelector(`[data-field="${name}"]`)?.textContent ?? null;
    const m = d.querySelector('[data-part=status]');
    const notice = d.querySelector('[data-notice]');
    return {
      id: d.dataset.sessionId,
      modal: d.matches(':modal'),
      title: d.querySelector('h2')?.textContent ?? null,
      status: m ? { word: m.querySelector('.word')?.textContent, glyph: m.querySelector('.glyph')?.textContent, tier: m.dataset.tier } : null,
      group: field('group'),
      location: field('location'),
      purpose: field('purpose'),
      configuration: field('configuration'),
      snapshot: field('snapshot'),
      age: field('age'),
      finalOutput: d.querySelector('[data-field="final-output"] pre')?.textContent ?? field('final-output'),
      finalOutputTruncated: !!d.querySelector('[data-part=final-output-truncated]'),
      reportedError: field('reported-error'),
      coverage: d.querySelector('[data-part=detail-coverage]')?.textContent ?? null,
      gaps: [...d.querySelectorAll('[data-part=detail-gaps] li')].map((li) => li.textContent),
      noLongerListed: !!d.querySelector('[data-part=no-longer-listed]'),
      notice: notice ? { kind: notice.dataset.notice, text: notice.textContent } : null,
    };
  });
}
