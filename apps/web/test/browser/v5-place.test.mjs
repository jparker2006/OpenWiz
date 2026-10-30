// V5: refresh keeps your place. Updates, inserts above the viewport, removals
// and page shifts keep the scroll position and the selection, and never
// duplicate or silently drop rows.

import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';

import { TIMING } from '../../src/timing.ts';
import { DESKTOP, launchBrowser, openPage, PHONE, readDetail, readView, waitFor } from '../support/browser.mjs';
import { html, json } from '../support/fake-api.mjs';
import { fakeSite } from '../support/harness.mjs';
import { inventory, manyRows, session, sessionView } from '../support/synthetic.ts';

const START = new Date('2027-01-15T08:00:00Z');

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(() => browser.close());

/** A fake API that serves `pages[i]` for the i-th list read (the last one after that). */
async function sequence(pages) {
  const fx = await fakeSite();
  let n = 0;
  fx.api.setList(() => json(200, pages[Math.min(n++, pages.length - 1)]));
  fx.api.setDetail((id) => json(200, sessionView(session(Number(id.slice(-4))))));
  return fx;
}

const ids = (view) => view.groups.flatMap((g) => g.rows.map((r) => r.id));
const top = (page, id) => page.$eval(`[data-session-id="${id}"]`, (el) => el.getBoundingClientRect().top);

async function refresh(p, fx) {
  const before = fx.api.reads('list').length;
  await p.page.clock.runFor(TIMING.listIntervalMs);
  await waitFor(() => fx.api.reads('list').length > before && fx.api.reads('list').at(-1).endedAt !== null, { what: 'a refresh' });
  await waitFor(async () => (await p.page.evaluate(() => document.querySelector('[data-snapshot-seq]')?.dataset.snapshotSeq)) === String(before + 1), {
    what: 'the refresh to render',
  });
}

/** Scrolls so `id` sits well inside the viewport, and returns its top. */
async function scrollTo(p, id) {
  await p.page.$eval(`[data-session-id="${id}"]`, (el) => window.scrollTo(0, el.getBoundingClientRect().top + window.scrollY - 300));
  return top(p.page, id);
}

const rows = (n, first = 1) => manyRows(n, first);

for (const [what, next] of [
  ['an update to rows on screen', (r) => r.map((s, i) => (i % 2 ? { ...s, status: { known: 'active' }, title: { known: `${s.title.known} (updated)` } } : s))],
  ['rows inserted above the viewport', (r) => [...rows(5, 500), ...r]],
  ['rows removed above the viewport', (r) => r.filter((s) => !['thr_web_0002', 'thr_web_0003', 'thr_web_0004'].includes(s.id))],
]) {
  test(`${what} keeps the scroll position`, async () => {
    for (const viewport of [PHONE, DESKTOP]) {
      const first = rows(60);
      const second = next(first);
      const fx = await sequence([inventory(first), inventory(second)]);
      const p = await openPage(browser, fx.site.origin, { clock: START, viewport });
      try {
        await waitFor(async () => ids(await readView(p.page)).length === 60, { what: 'rows' });
        const anchor = 'thr_web_0030';
        const y = await scrollTo(p, anchor);
        await refresh(p, fx);
        const view = await readView(p.page);
        assert.deepEqual(ids(view).toSorted(), second.map((s) => s.id).toSorted(), 'exactly the new page, each row once');
        assert.ok(Math.abs((await top(p.page, anchor)) - y) <= 1, `${viewport.width}px: the anchor row moved`);
      } finally {
        await p.close();
        await fx.close();
      }
    }
  });
}

test('a failed refresh that brings a notice above the rows keeps the scroll position', async () => {
  for (const viewport of [PHONE, DESKTOP]) {
    const fx = await fakeSite();
    let n = 0;
    fx.api.setList(() => (n++ === 0 ? json(200, inventory(rows(60))) : html(530, 'error code: 1033')));
    const p = await openPage(browser, fx.site.origin, { clock: START, viewport });
    try {
      await waitFor(async () => ids(await readView(p.page)).length === 60, { what: 'rows' });
      const anchor = 'thr_web_0030';
      const y = await scrollTo(p, anchor);
      await p.page.clock.runFor(TIMING.listIntervalMs);
      await waitFor(async () => (await readView(p.page)).connection === 'disconnected', { what: 'the failure' });
      assert.ok(Math.abs((await top(p.page, anchor)) - y) <= 1, `${viewport.width}px: the notice moved the rows`);
    } finally {
      await p.close();
      await fx.close();
    }
  }
});

test('the selection is kept through refreshes, and kept (marked) when its row leaves', async () => {
  for (const viewport of [PHONE, DESKTOP]) {
    const first = rows(20);
    const fx = await sequence([inventory(first), inventory(first), inventory(first.filter((s) => s.id !== 'thr_web_0010'))]);
    const p = await openPage(browser, fx.site.origin, { clock: START, viewport });
    try {
      await waitFor(async () => ids(await readView(p.page)).length === 20, { what: 'rows' });
      await p.page.click('[data-session-id="thr_web_0010"]');
      await waitFor(async () => (await readDetail(p.page))?.title === 'Synthetic web session 10', { what: 'the detail' });
      await refresh(p, fx);
      let d = await readDetail(p.page);
      assert.equal(d?.id, 'thr_web_0010', 'still open after a refresh');
      assert.equal(d.noLongerListed, false);
      await refresh(p, fx);
      d = await readDetail(p.page);
      assert.equal(d?.id, 'thr_web_0010', 'not silently closed');
      assert.equal(d.noLongerListed, true, 'shown as no longer listed');
      assert.equal(d.title, 'Synthetic web session 10', 'its details are kept');
      assert.equal(ids(await readView(p.page)).includes('thr_web_0010'), false);
    } finally {
      await p.close();
      await fx.close();
    }
  }
});

test('rows shifting across the page boundary are on the next page, never doubled or dropped', async () => {
  // 250 sessions, then 3 new ones at the top push the last three of page 1 onto page 2.
  const all = rows(250);
  const shifted = [...rows(3, 900), ...all];
  let source = all;
  const fx = await fakeSite();
  fx.api.setList((url) => {
    const offset = Number(url.searchParams.get('offset') ?? 0);
    return json(200, inventory(source.slice(offset, offset + 100), { offset, limit: 100, matched: source.length }));
  });
  const p = await openPage(browser, fx.site.origin, { clock: START });
  try {
    await waitFor(async () => (await readView(p.page)).range === '1–100 of 250', { what: 'page 1' });
    source = shifted;
    await refresh(p, fx);
    let view = await readView(p.page);
    assert.equal(view.range, '1–100 of 253');
    assert.deepEqual(ids(view).toSorted(), shifted.slice(0, 100).map((s) => s.id).toSorted());
    await p.page.click('[data-part=next]');
    await waitFor(async () => (await readView(p.page)).range === '101–200 of 253', { what: 'page 2' });
    view = await readView(p.page);
    const page2 = ids(view);
    assert.deepEqual(page2.toSorted(), shifted.slice(100, 200).map((s) => s.id).toSorted());
    for (const moved of ['thr_web_0098', 'thr_web_0099', 'thr_web_0100']) assert.ok(page2.includes(moved), `${moved} is on page 2`);
    assert.equal(new Set(page2).size, page2.length);
  } finally {
    await p.close();
    await fx.close();
  }
});

test('paging with the panel open keeps it, marked "no longer listed on this page"', async () => {
  const all = rows(250);
  const fx = await fakeSite();
  fx.api.setList((url) => {
    const offset = Number(url.searchParams.get('offset') ?? 0);
    return json(200, inventory(all.slice(offset, offset + 100), { offset, limit: 100, matched: all.length }));
  });
  fx.api.setDetail((id) => json(200, sessionView(session(Number(id.slice(-4))))));
  const p = await openPage(browser, fx.site.origin, { clock: START, viewport: DESKTOP });
  try {
    await waitFor(async () => (await readView(p.page)).range === '1–100 of 250', { what: 'page 1' });
    await p.page.click('[data-session-id="thr_web_0010"]');
    await waitFor(async () => (await readDetail(p.page))?.title === 'Synthetic web session 10', { what: 'the panel' });
    await p.page.click('[data-part=next]');
    await waitFor(async () => (await readView(p.page)).range === '101–200 of 250', { what: 'page 2' });
    const d = await readDetail(p.page);
    assert.equal(d?.id, 'thr_web_0010', 'not silently closed');
    assert.equal(d.noLongerListed, true);
  } finally {
    await p.close();
    await fx.close();
  }
});

test('a page read after the list changed says so: offset paging over a live list can miss or repeat a row', async () => {
  let source = rows(250);
  const fx = await fakeSite();
  fx.api.setList((url) => {
    const offset = Number(url.searchParams.get('offset') ?? 0);
    return json(200, inventory(source.slice(offset, offset + 100), { offset, limit: 100, matched: source.length }));
  });
  const p = await openPage(browser, fx.site.origin, { clock: START, viewport: DESKTOP });
  try {
    await waitFor(async () => (await readView(p.page)).range === '1–100 of 250', { what: 'page 1' });
    const note = () => p.page.evaluate(() => document.querySelector('[data-part=paging-note]')?.textContent ?? null);
    assert.match((await note()) ?? '', /separate snapshot/, 'every paged view says pages are separate snapshots');
    source = source.slice(1); // session 1 goes away before page 2 is read
    await p.page.click('[data-part=next]');
    await waitFor(async () => (await readView(p.page)).range === '101–200 of 249', { what: 'page 2' });
    const view = await readView(p.page);
    // The page shows exactly what the core returned: session 101 moved to page 1 and is on neither page shown.
    assert.deepEqual(ids(view).toSorted(), source.slice(100, 200).map((s) => s.id).toSorted());
    assert.equal(ids(view).includes('thr_web_0101'), false);
    const change = await p.page.evaluate(() => document.querySelector('[data-part=page-change]')?.textContent ?? null);
    assert.ok(change && change.includes('250') && change.includes('249'), `the change between the two pages is shown: ${change}`);
  } finally {
    await p.close();
    await fx.close();
  }
});
