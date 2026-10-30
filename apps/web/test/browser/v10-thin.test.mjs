// V10: a thin client. Only same-origin requests, to the API and the app's own
// files; no service worker, install manifest or third-party asset in the build.
// (The source-level half is `npm run check`.)

import assert from 'node:assert/strict';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { extname, join, relative } from 'node:path';
import { after, before, test } from 'node:test';

import { DESKTOP, launchBrowser, openPage, readDetail, readView, waitFor } from '../support/browser.mjs';
import { json } from '../support/fake-api.mjs';
import { fakeSite } from '../support/harness.mjs';
import { OUT_DIR } from '../support/paths.mjs';
import { inventory, manyRows, session, sessionView } from '../support/synthetic.ts';

let browser;
let fx;
before(async () => {
  browser = await launchBrowser();
  fx = await fakeSite();
  const all = manyRows(150);
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

const SAME_ORIGIN_PATHS = /^\/($|_next\/static\/[A-Za-z0-9._/-]+$|fonts\/[A-Za-z0-9._-]+\.woff2$|api\/v1\/inventory$|api\/v1\/sessions\/[A-Za-z0-9_-]+$)/;

test('every request the page makes is same-origin, to the API or its own files', async () => {
  const p = await openPage(browser, fx.site.origin, { viewport: DESKTOP });
  try {
    await waitFor(async () => (await readView(p.page)).groups.length > 0, { what: 'rows' });
    await p.page.click('[data-session-id="thr_web_0005"]');
    await waitFor(async () => (await readDetail(p.page))?.title === 'Synthetic web session 5', { what: 'a detail' });
    await p.page.click('[data-part=next]');
    await waitFor(async () => (await readView(p.page)).range === '101–150 of 150', { what: 'page 2' });
    assert.ok(p.requests.length > 3);
    for (const r of p.requests) {
      const url = new URL(r.url);
      assert.equal(url.origin, fx.site.origin, `a request to another origin: ${r.url}`);
      assert.match(url.pathname, SAME_ORIGIN_PATHS, `an unexpected path: ${url.pathname}`);
      assert.equal(r.method, 'GET');
    }
    const workers = await p.page.evaluate(async () => (navigator.serviceWorker ? (await navigator.serviceWorker.getRegistrations()).length : 0));
    assert.equal(workers, 0, 'no service worker');
  } finally {
    await p.close();
  }
});

function files(dir) {
  return readdirSync(dir).flatMap((f) => (statSync(join(dir, f)).isDirectory() ? files(join(dir, f)) : [join(dir, f)]));
}

/**
 * Absolute URLs that appear as inert text inside React's and Next's own
 * bundles (error-message links, XML namespaces). None is ever requested: the
 * request test above proves it. Each is listed here.
 */
export const INERT_URLS = [
  /^https:\/\/react\.dev\/errors\/$/,
  /^https:\/\/react\.dev\/link\/[a-z-]+$/,
  /^https:\/\/nextjs\.org\/docs\/messages\/([a-z0-9-]+)?$/,
  /^https:\/\/nextjs\.org\/docs\/app\/api-reference\/functions\/[a-z0-9-]+$/,
  // core-js's licence banner, inside Next's polyfills chunk (loaded only by browsers without modules).
  /^https:\/\/github\.com\/zloirock\/core-js(\/blob\/v[0-9.]+\/LICENSE)?$/,
  /^https?:\/\/www\.w3\.org\/(2000\/svg|1999\/xhtml|1998\/Math\/MathML|1999\/xlink|XML\/1998\/namespace)$/,
];

test('the build has no manifest, no service worker and no third-party asset', () => {
  const all = files(OUT_DIR);
  assert.ok(all.some((f) => f.endsWith('index.html')), 'the static export exists (npm run build)');
  const names = all.map((f) => relative(OUT_DIR, f));
  for (const name of names) {
    assert.match(extname(name), /^\.(html|js|css|woff2|txt)$/, `an unexpected file type: ${name}`);
    assert.doesNotMatch(name, /manifest\.json$|\.webmanifest$|(^|\/)sw\.js$|service-?worker/i, name);
  }
  const found = new Set();
  for (const f of all.filter((x) => /\.(html|js|css)$/.test(x))) {
    const text = readFileSync(f, 'utf8');
    assert.doesNotMatch(text, /serviceWorker\.register|rel="manifest"|importScripts\(/, relative(OUT_DIR, f));
    for (const m of text.matchAll(/(https?:)?\/\/[A-Za-z0-9.-]+\.[A-Za-z]{2,}(\/[A-Za-z0-9._~%/-]*)?/g)) {
      found.add(m[0]);
    }
    if (f.endsWith('.html')) {
      for (const m of text.matchAll(/\s(?:src|href)="([^"]*)"/g)) {
        assert.match(m[1], /^\/($|(_next\/static|fonts)\/)/, `${relative(OUT_DIR, f)}: a non-local reference ${m[1]}`);
      }
    }
  }
  const unexplained = [...found].filter((u) => !INERT_URLS.some((re) => re.test(u)));
  assert.deepEqual(unexplained, [], 'absolute URLs in the build that are not known inert text');
});
