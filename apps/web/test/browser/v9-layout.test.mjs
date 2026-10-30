// V9: layout. A bottom sheet at phone widths and a side panel at desktop
// widths, 44-point targets, a visible focus, keyboard use on desktop, and
// screen-reader names for every status and for the sheet or panel.

import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync } from 'node:fs';
import { after, before, test } from 'node:test';

import { DESKTOP, launchBrowser, openPage, PHONE, readDetail, readView, waitFor } from '../support/browser.mjs';
import { html, json } from '../support/fake-api.mjs';
import { axe, fakeSite } from '../support/harness.mjs';
import { RESULTS_DIR } from '../support/paths.mjs';
import { inventory, manyRows, session, sessionView, statusSessions } from '../support/synthetic.ts';

let browser;
let fx;
before(async () => {
  browser = await launchBrowser();
  fx = await fakeSite();
  fx.api.setList(() =>
    json(200, inventory([...statusSessions(), ...manyRows(30, 10)], { state: 'partial', matched: 300, limit: 100, gaps: [{ kind: 'timed_out', subject: null, detail: 'machine list timed out' }] })),
  );
  fx.api.setDetail((id) => json(200, sessionView(session(Number(id.slice(-4))))));
});
after(async () => {
  await fx.close();
  await browser.close();
});

async function ready(viewport) {
  const p = await openPage(browser, fx.site.origin, { viewport });
  await waitFor(async () => (await readView(p.page)).groups.length > 0, { what: 'rows' });
  return p;
}

async function open(p, id = 'thr_web_0002') {
  await p.page.click(`[data-session-id="${id}"]`);
  await waitFor(async () => (await readDetail(p.page))?.title?.startsWith('Synthetic'), { what: 'the detail' });
}

const rect = (page, sel) => page.$eval(sel, (el) => el.getBoundingClientRect().toJSON());

test('phone: a modal bottom sheet', async () => {
  const p = await ready(PHONE);
  try {
    await open(p);
    const d = await readDetail(p.page);
    assert.equal(d.modal, true);
    const r = await rect(p.page, '[data-part=detail]');
    assert.ok(Math.abs(r.bottom - PHONE.height) <= 1, `at the bottom (${r.bottom})`);
    assert.ok(Math.abs(r.left) <= 1 && Math.abs(r.width - PHONE.width) <= 1, 'full width');
    assert.ok(r.height < PHONE.height, 'not the whole screen');
  } finally {
    await p.close();
  }
});

test('desktop: a side panel on the right, with the list still usable', async () => {
  const p = await ready(DESKTOP);
  try {
    await open(p);
    const d = await readDetail(p.page);
    assert.equal(d.modal, false);
    const r = await rect(p.page, '[data-part=detail]');
    assert.ok(Math.abs(r.right - DESKTOP.width) <= 1, `on the right (${r.right})`);
    assert.ok(r.width >= 360 && r.width <= 480, `a panel width (${r.width})`);
    await open(p, 'thr_web_0003');
    assert.equal((await readDetail(p.page)).id, 'thr_web_0003', 'another row opens in the same panel');
    const list = await rect(p.page, '[data-session-id="thr_web_0001"]');
    assert.ok(list.right <= r.left + 1, 'the panel does not cover the rows');
  } finally {
    await p.close();
  }
});

test('the switch is at 900 px', async () => {
  for (const [width, modal] of [
    [899, true],
    [900, false],
  ]) {
    const p = await ready({ width, height: 800 });
    try {
      await open(p);
      assert.equal((await readDetail(p.page)).modal, modal, `${width}px`);
    } finally {
      await p.close();
    }
  }
});

async function smallTargets(page) {
  return page.evaluate(() =>
    [...document.querySelectorAll('button, a[href], summary, [tabindex]:not([tabindex="-1"]), input, select, textarea')]
      .filter((el) => {
        const r = el.getBoundingClientRect();
        return r.width > 0 && r.height > 0 && getComputedStyle(el).visibility !== 'hidden';
      })
      .map((el) => ({ el: el.outerHTML.slice(0, 80), r: el.getBoundingClientRect() }))
      .filter(({ r }) => r.width < 44 || r.height < 44)
      .map(({ el, r }) => `${el} ${Math.round(r.width)}x${Math.round(r.height)}`),
  );
}

test('every touch target is at least 44 × 44', async () => {
  for (const viewport of [PHONE, DESKTOP]) {
    const p = await ready(viewport);
    try {
      await p.page.click('[data-part=coverage]');
      assert.deepEqual(await smallTargets(p.page), [], `list at ${viewport.width}px`);
      await open(p);
      assert.deepEqual(await smallTargets(p.page), [], `detail at ${viewport.width}px`);
    } finally {
      await p.close();
    }
  }
});

test('keyboard: Tab to a row, Enter opens, focus moves in, Escape closes, focus returns', async () => {
  for (const viewport of [DESKTOP, PHONE]) {
    const p = await ready(viewport);
    try {
      let row = null;
      for (let i = 0; i < 40 && !row; i++) {
        await p.page.keyboard.press('Tab');
        row = await p.page.evaluate(() => document.activeElement?.dataset?.sessionId ?? null);
      }
      assert.ok(row, 'a row is reachable by Tab');
      const ring = await p.page.evaluate(() => {
        const s = getComputedStyle(document.activeElement);
        return { style: s.outlineStyle, width: parseFloat(s.outlineWidth) };
      });
      assert.ok(ring.style !== 'none' && ring.width >= 2, `a visible focus ring (${JSON.stringify(ring)})`);
      await p.page.keyboard.press('Enter');
      await waitFor(async () => (await readDetail(p.page))?.id === row, { what: 'the detail by keyboard' });
      const inside = await p.page.evaluate(() => !!document.activeElement?.closest('[data-part=detail]'));
      assert.ok(inside, 'focus moves into the sheet or panel');
      await p.page.keyboard.press('Escape');
      await waitFor(async () => (await readDetail(p.page)) === null, { what: 'Escape closes' });
      const back = await p.page.evaluate(() => document.activeElement?.dataset?.sessionId ?? null);
      assert.equal(back, row, 'focus returns to the row');
    } finally {
      await p.close();
    }
  }
});

test('screen-reader names: every row says its status word; the sheet or panel is named', async () => {
  const p = await ready(PHONE);
  try {
    const tree = await p.page.locator('main').ariaSnapshot();
    // Each row's own accessible name has its own status word.
    for (const [id, word] of [
      ['thr_web_0001', 'error'],
      ['thr_web_0002', 'active'],
      ['thr_web_0003', 'idle'],
      ['thr_web_0004', 'syn-other'],
      ['thr_web_0005', 'unknown (missing field status)'],
    ]) {
      const name = await p.page.locator(`[data-session-id="${id}"]`).ariaSnapshot();
      assert.match(name, /^- button "/, `${id} is a button`);
      assert.ok(name.includes(` ${word}"`) || name.includes(` ${word} `), `${id}'s name has "${word}": ${name}`);
    }
    assert.doesNotMatch(tree, /\[~\]|\[x\]|\[-\]|\[\?\]/, 'the glyphs are not read out');
    await open(p);
    const name = await p.page.$eval('[data-part=detail]', (d) => document.getElementById(d.getAttribute('aria-labelledby') ?? '')?.textContent);
    assert.equal(name, 'Synthetic web session 2');
    mkdirSync(RESULTS_DIR, { recursive: true });
    writeFileSync(`${RESULTS_DIR}/v9-aria-list.txt`, `${tree}\n`);
    writeFileSync(`${RESULTS_DIR}/v9-aria-detail.txt`, `${await p.page.locator('[data-part=detail]').ariaSnapshot()}\n`);
  } finally {
    await p.close();
  }
});

test('axe finds no violation: list, gaps, sheet, panel and a failure state', async () => {
  for (const viewport of [PHONE, DESKTOP]) {
    const p = await ready(viewport);
    try {
      assert.deepEqual(await axe(p.page), [], `list at ${viewport.width}px`);
      await p.page.click('[data-part=coverage]');
      assert.deepEqual(await axe(p.page), [], `gaps at ${viewport.width}px`);
      await open(p);
      assert.deepEqual(await axe(p.page), [], `detail at ${viewport.width}px`);
    } finally {
      await p.close();
    }
  }
  const down = await fakeSite();
  down.api.setList(() => html(530, 'error code: 1033'));
  const q = await openPage(browser, down.site.origin, { viewport: PHONE });
  try {
    await waitFor(async () => (await readView(q.page)).connection === 'disconnected', { what: 'disconnected' });
    assert.deepEqual(await axe(q.page), [], 'disconnected');
  } finally {
    await q.close();
    await down.close();
  }
});
