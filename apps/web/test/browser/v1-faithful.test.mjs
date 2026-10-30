// V1: faithful to the envelope. The real openwiz-server runs over every
// 0.0.1 fixture scenario; the page must show every row once, under the group
// the envelope gives it, and every word, unknown and truncation word for word.

import assert from 'node:assert/strict';
import { after, before, describe, test } from 'node:test';

import { launchBrowser, openPage, readDetail, readView, waitFor } from '../support/browser.mjs';
import * as expect from '../support/expect.mjs';
import { scenarios, startFixtureServer } from '../support/rust.mjs';
import { startSite } from '../support/serve.mjs';

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(() => browser.close());

const listResponse = (p) => p.responses.find((r) => r.path === '/api/v1/inventory' && r.body !== null);

describe('V1 over every 0.0.1 fixture scenario, through the real server', { concurrency: 6 }, () => {
  for (const name of scenarios()) {
    test(name, { timeout: 180_000 }, async () => {
      const server = await startFixtureServer(name);
      const site = await startSite({ port: server.port });
      const p = await openPage(browser, site.origin);
      try {
        const res = await waitFor(() => listResponse(p), { timeout: 60_000, what: 'the list response' });
        await waitFor(async () => (await readView(p.page)).connection !== 'reading', { what: 'the list to render' });
        const body = JSON.parse(res.body);
        const view = await readView(p.page);

        if (res.status !== 200) {
          assert.ok(view.notice, 'an error notice is shown');
          assert.ok(view.notice.text.includes(body.error.kind), `the notice names ${body.error.kind}`);
          assert.ok(view.notice.text.includes(body.error.message), 'the notice has the message word for word');
          assert.deepEqual(view.groups, [], 'no rows without an envelope');
          return;
        }

        assert.deepEqual(view.groups, expect.groups(body.data.sessions), 'rows under their groups, word for word');
        const ids = view.groups.flatMap((g) => g.rows.map((r) => r.id));
        assert.equal(new Set(ids).size, ids.length, 'every row once');
        assert.equal(ids.length, body.data.sessions.length, 'no row dropped');
        assert.equal(view.coverage, expect.coverage(body));
        if (body.gaps.length > 0) {
          await p.page.click('[data-part=coverage]');
          assert.deepEqual((await readView(p.page)).gaps, body.gaps.map(expect.gapLine), 'the gaps, word for word');
        }

        // Every session's detail, as `show` returns it.
        for (const s of body.data.sessions) {
          const path = `/api/v1/sessions/${s.id}`;
          const seen = p.responses.length;
          await p.page.click(`[data-session-id="${s.id}"]`);
          const shown = await waitFor(() => p.responses.slice(seen).find((r) => r.path === path && r.body !== null), {
            timeout: 60_000,
            what: `the detail of ${s.id}`,
          });
          const detailBody = JSON.parse(shown.body);
          const d = await waitFor(async () => {
            const x = await readDetail(p.page);
            return x && x.id === s.id && (x.title !== null || x.notice) ? x : null;
          });
          if (shown.status === 200) {
            const now = await p.page.evaluate(() => Date.now());
            const want = expect.detail(detailBody, now);
            for (const key of Object.keys(want)) assert.deepEqual(d[key], want[key], `${s.id}: ${key}`);
          } else {
            assert.ok(d.notice?.text.includes(detailBody.error.message), `${s.id}: the core's message`);
          }
          await p.page.keyboard.press('Escape');
          await waitFor(async () => (await readDetail(p.page)) === null, { what: 'the detail to close' });
        }
        assert.deepEqual(await p.csp(), [], 'no CSP violation');
        assert.deepEqual(p.errors, [], 'no console error');
      } finally {
        await p.close();
        await site.close();
        await server.stop();
      }
    });
  }
});
