// AW7: safe previews. An SVG with a script, an HTML page, a polyglot
// (a PNG with markup after it) and a PNG named.svg never render as markup;
// thumbnails appear only for PNG, JPEG, GIF and WebP, from the browser's own
// copy (an object URL), and are revoked on removal. The page loads under its
// CSP with no violation, and nothing in a file ever runs. AW8: one tray, in
// the one prompt body, in both the composer and the launcher.

import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';

import { launchBrowser, waitFor } from '../support/browser.mjs';
import { composerSite, openThread, readComposer, type } from '../support/composer.mjs';
import * as at from '../support/attachment-synthetic.mjs';
import { launcherSite, openLauncher } from '../support/launcher.mjs';
import { addFiles, COMPOSER, gif, htmlWithScript, jpeg, LAUNCHER, png, polyglot, readTray, svgWithScript, text, trayIs, webp } from '../support/tray.mjs';

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(() => browser.close());

test('AW7: thumbnails only for the four image types, from an object URL; markup never renders', async () => {
  const { fx } = await composerSite();
  // The core refuses a PNG named.svg (the name and the bytes disagree); everything else is staged.
  fx.api.setUpload((name, bytes) => (name === 'png-as.svg' ? at.failed(name, 'extension_mismatch', 'its name says .svg but its bytes are a PNG') : at.ready(fx.api.staged, name, bytes)));
  const p = await openThread(browser, fx, 1);
  try {
    await type(p.page, 'Synthetic message');
    const files = [
      { name: 'a.png', buffer: png(), mimeType: 'image/png' },
      { name: 'b.jpg', buffer: jpeg(), mimeType: 'image/jpeg' },
      { name: 'c.gif', buffer: gif(), mimeType: 'image/gif' },
      { name: 'd.webp', buffer: webp(), mimeType: 'image/webp' },
      { name: 'e.svg', buffer: svgWithScript(), mimeType: 'image/svg+xml' },
      { name: 'f.html', buffer: htmlWithScript(), mimeType: 'text/html' },
      { name: 'polyglot.png', buffer: polyglot(), mimeType: 'image/png' },
      { name: 'png-as.svg', buffer: png(), mimeType: 'image/svg+xml' },
      { name: 'g.txt', buffer: text() },
    ];
    await addFiles(p.page, COMPOSER, files);
    const t = await waitFor(async () => {
      const t = await readTray(p.page, COMPOSER);
      return t?.rows.length === files.length && t.rows.every((r) => r.state === 'ready' || r.state === 'failed') ? t : null;
    }, { what: 'every upload answered' });
    const thumbs = Object.fromEntries(t.rows.map((r) => [r.name, r.thumb]));
    assert.deepEqual(thumbs, {
      'a.png': 'yes',
      'b.jpg': 'yes',
      'c.gif': 'yes',
      'd.webp': 'yes',
      'e.svg': 'no',
      'f.html': 'no',
      'polyglot.png': 'yes',
      'png-as.svg': 'no',
      'g.txt': 'no',
    });
    for (const r of t.rows) if (r.thumb === 'yes') assert.match(r.img, /^blob:/, `${r.name}: the browser's own copy`);
    for (const r of t.rows) if (r.thumb === 'no') assert.equal(r.img, null, `${r.name}: a glyph, never an image`);
    assert.equal(t.rows.find((r) => r.name === 'png-as.svg').state, 'failed');
    // Nothing ran, nothing framed, no other element shows a file, and the CSP saw nothing to block.
    const page = await p.page.evaluate(() => ({
      pwned: window.__pwned ?? null,
      frames: document.querySelectorAll('iframe, object, embed, frame').length,
      svgs: document.querySelectorAll('[data-part=tray] svg').length,
      images: [...document.querySelectorAll('img')].map((i) => i.getAttribute('src')).filter((s) => !s.startsWith('blob:')),
    }));
    assert.deepEqual(page, { pwned: null, frames: 0, svgs: 0, images: [] });
    assert.deepEqual(await p.csp(), []);
    // Removal revokes the object URL: the browser no longer resolves it.
    const url = t.rows.find((r) => r.name === 'a.png').img;
    await p.page.locator(`${COMPOSER} [data-part=attachment-remove][aria-label="Remove a.png"]`).click();
    await waitFor(async () => (await readTray(p.page, COMPOSER)).rows.length === files.length - 1, { what: 'removed' });
    const resolves = await p.page.evaluate(async (u) => {
      try {
        await fetch(u);
        return true;
      } catch {
        return false;
      }
    }, url);
    assert.equal(resolves, false, 'the object URL is revoked');
  } finally {
    await p.close();
    await fx.close();
  }
});

test('AW7: the server never sends a file back; every image on the page is the browser’s own object URL', async () => {
  const { fx } = await composerSite();
  const p = await openThread(browser, fx, 1);
  try {
    await addFiles(p.page, COMPOSER, [{ name: 'a.png', buffer: png(), mimeType: 'image/png' }]);
    await trayIs(p.page, COMPOSER, 1, 'ready');
    const api = p.requests.filter((r) => new URL(r.url).pathname.startsWith('/api/v1/attachments'));
    assert.deepEqual(api.map((r) => r.method), ['POST'], 'one upload; no read of a file back');
  } finally {
    await p.close();
    await fx.close();
  }
});

test('AW8: one tray, inside the one prompt body, in the composer and in the launcher', async () => {
  const { fx } = await composerSite();
  const p = await openThread(browser, fx, 1);
  try {
    const t = await readTray(p.page, COMPOSER);
    assert.equal(t.inPromptBody, true);
    assert.equal(await p.page.locator('[data-part=tray]').count(), 1);
    assert.equal(await p.page.locator('input[type=file]').count(), 1);
    assert.ok(await readComposer(p.page));
  } finally {
    await p.close();
    await fx.close();
  }
  const l = await launcherSite();
  const q = await openLauncher(browser, l.fx);
  try {
    const t = await readTray(q.page, LAUNCHER);
    assert.equal(t.inPromptBody, true);
    assert.equal(await q.page.locator('[data-part=tray]').count(), 1);
    assert.equal(await q.page.locator('input[type=file]').count(), 1);
  } finally {
    await q.close();
    await l.fx.close();
  }
});
