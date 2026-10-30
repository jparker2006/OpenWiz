// V3: untrusted text stays inert. HTML, script, links, markdown, bidirectional
// controls and over-long strings in every text field render as plain text;
// the built pages carry no inline script and load under a CSP without
// 'unsafe-inline'.

import assert from 'node:assert/strict';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { after, before, test } from 'node:test';

import { DESKTOP, launchBrowser, openPage, PHONE, readDetail, readView, waitFor } from '../support/browser.mjs';
import { json } from '../support/fake-api.mjs';
import * as expect from '../support/expect.mjs';
import { fakeSite } from '../support/harness.mjs';
import { OUT_DIR } from '../support/paths.mjs';
import { PAGE_CSP } from '../support/serve.mjs';
import { HOSTILE, hostileInventory, hostileSessions, hostileView } from '../support/synthetic.ts';

let browser;
let fx;
const sessions = hostileSessions();
before(async () => {
  browser = await launchBrowser();
  fx = await fakeSite();
  fx.api.setList(() => json(200, hostileInventory()));
  fx.api.setDetail((id) => json(200, hostileView(sessions.find((s) => s.id === id))));
});
after(async () => {
  await fx.close();
  await browser.close();
});

/** Nothing ran, and no element came from session text. */
async function assertInert(page) {
  const found = await page.evaluate(() => ({
    pwned: window.__pwned ?? null,
    imgs: document.querySelectorAll('img').length,
    jsLinks: document.querySelectorAll('a[href^="javascript:"]').length,
    handlers: [...document.querySelectorAll('*')].flatMap((el) => [...el.attributes].filter((a) => a.name.startsWith('on')).map((a) => a.name)),
    scripts: [...document.querySelectorAll('script')].filter((s) => !s.src && s.type !== 'application/json').length,
    bold: document.querySelectorAll('main b, main strong, [data-part=detail] b').length,
  }));
  assert.deepEqual(found, { pwned: null, imgs: 0, jsLinks: 0, handlers: [], scripts: 0, bold: 0 });
}

async function assertNoHorizontalOverflow(page, where) {
  const o = await page.evaluate(() => {
    const d = document.querySelector('[data-part=detail]');
    return {
      page: document.documentElement.scrollWidth - window.innerWidth,
      detail: d && d.open ? d.scrollWidth - d.clientWidth : 0,
    };
  });
  assert.ok(o.page <= 0, `${where}: the page overflows sideways by ${o.page}px`);
  assert.ok(o.detail <= 0, `${where}: the detail overflows sideways by ${o.detail}px`);
}

test('hostile text in every list field is shown as text, and runs nothing', async () => {
  for (const viewport of [PHONE, DESKTOP]) {
    const p = await openPage(browser, fx.site.origin, { viewport });
    try {
      await waitFor(async () => (await readView(p.page)).groups.length > 0, { what: 'rows' });
      const view = await readView(p.page);
      // Under their groups: the order the page shows is the grouped order.
      const want = expect.groups(sessions).flatMap((g) => g.rows);
      const rows = view.groups.flatMap((g) => g.rows);
      assert.deepEqual(
        rows.map((r) => r.title),
        want.map((r) => r.title),
      );
      assert.deepEqual(
        rows.map((r) => r.status.word),
        want.map((r) => r.status.word),
      );
      for (const g of view.groups) {
        if (g.key.startsWith('project:')) assert.ok(Object.values(HOSTILE).includes(g.heading), 'a hostile project name as text');
      }
      await p.page.click('[data-part=coverage]');
      assert.deepEqual(
        (await readView(p.page)).gaps,
        hostileInventory().gaps.map((g) => `${g.kind}${g.subject === null ? '' : ` · ${g.subject}`} · ${g.detail}`),
      );
      await assertInert(p.page);
      await assertNoHorizontalOverflow(p.page, `list at ${viewport.width}px`);
      assert.deepEqual(await p.csp(), []);
    } finally {
      await p.close();
    }
  }
});

test('hostile text in every detail field, and 4 KiB of output, is shown as text', async () => {
  for (const viewport of [PHONE, DESKTOP]) {
    const p = await openPage(browser, fx.site.origin, { viewport });
    try {
      await waitFor(async () => (await readView(p.page)).groups.length > 0, { what: 'rows' });
      for (const s of sessions) {
        await p.page.click(`[data-session-id="${s.id}"]`);
        const d = await waitFor(async () => {
          const x = await readDetail(p.page);
          return x && x.id === s.id && x.finalOutput && x.finalOutput.length > 100 ? x : null;
        });
        const view = hostileView(s);
        assert.equal(d.title, s.title.known);
        assert.equal(d.status.word, s.status.known);
        assert.equal(d.location, `local on ${s.location.known.machine.known}`);
        assert.equal(d.configuration, `model ${s.reported_configuration.model.known} · effort unknown (${s.reported_configuration.effort.unknown})`);
        assert.equal(d.finalOutput, `${view.data.final_output.known.text}…`);
        assert.equal(d.reportedError, view.data.reported_error.known.text);
        await assertInert(p.page);
        await assertNoHorizontalOverflow(p.page, `${s.id} at ${viewport.width}px`);
        await p.page.keyboard.press('Escape');
        await waitFor(async () => (await readDetail(p.page)) === null, { what: 'the detail to close' });
      }
      assert.deepEqual(await p.csp(), []);
      assert.deepEqual(p.errors, []);
    } finally {
      await p.close();
    }
  }
});

test('bidirectional controls in session text cannot reorder the page around it', async () => {
  const p = await openPage(browser, fx.site.origin, { viewport: PHONE });
  try {
    await waitFor(async () => (await readView(p.page)).groups.length > 0, { what: 'rows' });
    const bidi = sessions.find((s) => s.title.known === HOSTILE.bidi);
    const layout = await p.page.evaluate((id) => {
      const row = document.querySelector(`[data-session-id="${id}"]`);
      const title = row.querySelector('[data-part=title]');
      const isolated = (el) => {
        for (let e = el; e && e !== row; e = e.parentElement) {
          const u = getComputedStyle(e).unicodeBidi;
          if (u === 'isolate' || u === 'plaintext' || u === 'isolate-override') return true;
        }
        return false;
      };
      const textNodes = [];
      const walk = document.createTreeWalker(document.querySelector('main'), NodeFilter.SHOW_TEXT);
      for (let n = walk.nextNode(); n; n = walk.nextNode()) if (/[‪-‮⁦-⁩]/.test(n.data)) textNodes.push(n);
      const r = row.getBoundingClientRect();
      const age = row.querySelector('[data-part=age]').getBoundingClientRect();
      return {
        allIsolated: textNodes.length > 0 && textNodes.every((n) => isolated(n.parentElement)),
        titleIsolated: (() => {
          const text = document.createTreeWalker(title, NodeFilter.SHOW_TEXT).nextNode();
          return !!text && isolated(text.parentElement);
        })(),
        ageAtRight: Math.abs(r.right - age.right) < 24,
      };
    }, bidi.id);
    assert.deepEqual(layout, { allIsolated: true, titleIsolated: true, ageAtRight: true });
  } finally {
    await p.close();
  }
});

function files(dir) {
  return readdirSync(dir).flatMap((f) => (statSync(join(dir, f)).isDirectory() ? files(join(dir, f)) : [join(dir, f)]));
}

test('the built pages have no inline script, handler or style attribute', () => {
  const html = files(OUT_DIR).filter((f) => f.endsWith('.html'));
  assert.ok(html.length > 0, 'the static export exists (npm run build)');
  for (const f of html) {
    const text = readFileSync(f, 'utf8');
    const scripts = [...text.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script>/g)];
    for (const [, attrs, body] of scripts) {
      const external = /\ssrc="\/_next\/static\/[^"]+"/.test(attrs);
      const data = /\stype="application\/json"/.test(attrs);
      assert.ok(external || data, `${f}: an inline script: <script${attrs}>${body.slice(0, 60)}`);
      if (external) assert.equal(body.trim(), '', `${f}: an external script with a body`);
    }
    assert.doesNotMatch(text, /\son[a-z]+=/i, `${f}: an inline event handler`);
    assert.doesNotMatch(text, /\sstyle=/i, `${f}: a style attribute`);
    assert.doesNotMatch(text, /<style\b/i, `${f}: an inline style element`);
    assert.doesNotMatch(text, /javascript:/i, `${f}: a javascript: URL`);
  }
});

test('the page CSP has no unsafe source, and the site serves it', async () => {
  assert.doesNotMatch(PAGE_CSP, /unsafe|data:|blob:|\*/);
  assert.match(PAGE_CSP, /script-src 'self'/);
  const res = await fetch(`${fx.site.origin}/`);
  assert.equal(res.headers.get('content-security-policy'), PAGE_CSP);
});

test('the emitted JavaScript has no eval or Function(...), except the recorded noModule polyfills exception', () => {
  const html = readFileSync(join(OUT_DIR, 'index.html'), 'utf8');
  const noModule = [...html.matchAll(/<script\b[^>]*\snoModule=""[^>]*\ssrc="([^"]+)"/g)].map((m) => m[1]);
  const js = files(join(OUT_DIR, '_next')).filter((f) => f.endsWith('.js'));
  assert.ok(js.length > 0);
  const found = [];
  for (const f of js) {
    const text = readFileSync(f, 'utf8');
    if (/\beval\s*\(|\bFunction\s*\(/.test(text)) found.push(f.slice(OUT_DIR.length));
  }
  // The only allowed file is the polyfills chunk that pages load with `noModule`:
  // no browser that runs ES modules (every supported one) loads it.
  assert.deepEqual(found.filter((f) => !noModule.includes(f)), [], 'eval or Function(...) in a chunk the page runs');
});
