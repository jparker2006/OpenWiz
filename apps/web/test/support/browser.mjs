// The browser for the proofs: a local Chromium (`OPENWIZ_CHROMIUM`, else
// /usr/bin/chromium), driven by playwright-core. Every page records its requests, console errors and
// Content Security Policy violations.

import { chromium } from 'playwright-core';

export const PHONE = { width: 390, height: 844 };
export const DESKTOP = { width: 1440, height: 900 };

export function launchBrowser() {
  return chromium.launch({ executablePath: process.env.OPENWIZ_CHROMIUM ?? '/usr/bin/chromium' });
}

/**
 * Opens `origin` in a fresh context. `clock`: install Playwright's fake clock
 * at that time before the page loads.
 */
export async function openPage(browser, origin, { viewport = PHONE, clock = null, path = '/', hasTouch = false } = {}) {
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
  if (clock !== null) await page.clock.install({ time: clock });
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

/** The page as the proofs read it. */
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
