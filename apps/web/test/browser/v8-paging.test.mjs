// V8: paging stays within the contract. Only `limit` and `offset` exist; the
// page sends `offset` alone (the core's default limit), never above the core's
// cap, and no cursor, revision or other parameter.

import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';

import { launchBrowser, openPage, readDetail, readView, waitFor } from '../support/browser.mjs';
import { json } from '../support/fake-api.mjs';
import { fakeSite } from '../support/harness.mjs';
import { inventory, manyRows, session, sessionView } from '../support/synthetic.ts';

const all = manyRows(250);

let browser;
let fx;
before(async () => {
  browser = await launchBrowser();
  fx = await fakeSite();
  fx.api.setList((url) => {
    const offset = Number(url.searchParams.get('offset') ?? 0);
    return json(200, inventory(all.slice(offset, offset + 100), { offset, limit: 100, matched: all.length }));
  });
  fx.api.setDetail((id) => json(200, sessionView(session(Number(id.slice(-4))))));
});
after(async () => {
  await fx.close();
  await browser.close();
});

const ALLOWED_LIST_QUERIES = /^(|\?offset=(0|[1-9][0-9]*))$/;

test('next, next, previous: offsets from the envelope\'s own counts, nothing else', async () => {
  const p = await openPage(browser, fx.site.origin);
  try {
    await waitFor(async () => (await readView(p.page)).range === '1–100 of 250', { what: 'page 1' });
    await p.page.click('[data-part=next]');
    await waitFor(async () => (await readView(p.page)).range === '101–200 of 250', { what: 'page 2' });
    await p.page.click('[data-part=next]');
    await waitFor(async () => (await readView(p.page)).range === '201–250 of 250', { what: 'page 3' });
    assert.equal(await p.page.$('[data-part=next]:not([disabled])'), null, 'no page past the end');
    await p.page.click('[data-part=previous]');
    await waitFor(async () => (await readView(p.page)).range === '101–200 of 250', { what: 'back to page 2' });
    await p.page.click('[data-session-id="thr_web_0150"]');
    await waitFor(async () => (await readDetail(p.page))?.title === 'Synthetic web session 150', { what: 'a detail' });

    const lists = fx.api.reads('list');
    const firstOfEach = [...new Set(lists.map((e) => e.search))];
    assert.deepEqual(firstOfEach, ['', '?offset=100', '?offset=200']);
    for (const e of fx.api.log) {
      assert.equal(e.method, 'GET');
      if (e.kind === 'list') assert.match(e.search, ALLOWED_LIST_QUERIES, `list query ${e.search}`);
      else if (e.kind === 'detail') {
        assert.equal(e.search, '', 'a detail read takes no query');
        assert.match(e.path, /^\/api\/v1\/sessions\/[A-Za-z0-9_-]{1,128}$/);
      } else assert.fail(`a request outside the contract: ${e.path}`);
      assert.doesNotMatch(e.path + e.search, /cursor|revision|@r|limit|since|after/i);
      assert.equal(e.headers['x-requested-with'], 'XMLHttpRequest');
    }
  } finally {
    await p.close();
  }
});
