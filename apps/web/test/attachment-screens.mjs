// Renders DEMO-labelled screenshots of the attachment tray's states  at
// 390 and 1440 px, in the composer and the launcher, on the fake API:
// `node test/attachment-screens.mjs [out-dir]`. Not a proof, and not the maintainer
// acceptance: his design loop is on the working site. Synthetic files only.

import { mkdirSync } from 'node:fs';

import { DESKTOP, launchBrowser, PHONE, waitFor } from './support/browser.mjs';
import * as at from './support/attachment-synthetic.mjs';
import { composerSite, id, openThread, type } from './support/composer.mjs';
import { HANG } from './support/fake-api.mjs';
import { chooseAll, launcherSite, openLauncher, typePrompt } from './support/launcher.mjs';
import { RESULTS_DIR } from './support/paths.mjs';
import { addFiles, COMPOSER, LAUNCHER, png, readTray, svgWithScript, text } from './support/tray.mjs';

const dir = process.argv[2] ?? `${RESULTS_DIR}/attachment-screens`;
mkdirSync(dir, { recursive: true });

/** A DEMO ribbon, added outside the page's CSP, so no screenshot passes for real data. */
const demo = (page) =>
  page.evaluate(() => {
    const r = document.createElement('div');
    r.textContent = 'DEMO · SYNTHETIC DATA · NOTHING IS LIVE';
    Object.assign(r.style, {
      position: 'fixed', left: '0', right: '0', bottom: '0', zIndex: '100', padding: '4px 16px', textAlign: 'center',
      background: '#552583', color: '#e8eaed', font: '600 10px/16px GeistMono, monospace', letterSpacing: '.08em',
    });
    (document.querySelector('dialog[open]:modal') ?? document.body).append(r);
  });

/** Every state at once: a ready image with its thumbnail, a ready file, one uploading, one failed with the core's words, an SVG as a glyph. */
function mixed(fx) {
  let n = 0;
  fx.api.setUpload((name, bytes) => {
    n++;
    if (name.startsWith('uploading')) return HANG;
    if (name.endsWith('.heic')) return at.failed(name, 'unsupported_heif', 'HEIC/HEIF images aren\'t supported. Nothing was converted');
    return at.ready(fx.api.staged, name, bytes);
  });
  return () => n;
}

const FILES = [
  { name: 'synthetic-photo.png', buffer: png(), mimeType: 'image/png' },
  { name: 'synthetic-meeting-notes-with-a-rather-long-name-2026-10-31.txt', buffer: text() },
  { name: 'uploading-synthetic-report.txt', buffer: text('Synthetic report\n') },
  { name: 'synthetic-camera.heic', buffer: text('synthetic') },
  { name: 'synthetic-diagram.svg', buffer: svgWithScript(), mimeType: 'image/svg+xml' },
];

const settledMixed = (page, scope) =>
  waitFor(async () => {
    const t = await readTray(page, scope);
    return t?.rows.length === FILES.length && t.rows.filter((r) => r.state === 'uploading').length === 1 && t.rows.every((r) => r.state !== 'selected') ? t : null;
  }, { what: 'the mixed tray' });

const browser = await launchBrowser();
try {
  for (const [size, viewport] of [
    ['390', PHONE],
    ['1440', DESKTOP],
  ]) {
    // The composer: the tray with every state, Send off and why.
    {
      const { fx } = await composerSite();
      mixed(fx);
      const p = await openThread(browser, fx, 1, { viewport });
      try {
        await type(p.page, 'Synthetic message: reply ok');
        await addFiles(p.page, COMPOSER, FILES);
        await settledMixed(p.page, COMPOSER);
        await demo(p.page);
        await p.page.locator(`${COMPOSER} [data-part=tray]`).scrollIntoViewIfNeeded();
        await p.page.screenshot({ path: `${dir}/composer-tray-states-${size}.png`, fullPage: false });
      } finally {
        await p.close();
        await fx.close();
      }
    }
    // The composer of a busy thread with a ready file: no Steer, and why.
    {
      const { fx } = await composerSite({ kinds: { [id(1)]: 'busy' } });
      const p = await openThread(browser, fx, 1, { viewport });
      try {
        await type(p.page, 'Synthetic message: reply ok');
        await addFiles(p.page, COMPOSER, [FILES[0], FILES[1]]);
        await waitFor(async () => (await readTray(p.page, COMPOSER))?.rows.every((r) => r.state === 'ready'), { what: 'ready' });
        await demo(p.page);
        await p.page.locator(`${COMPOSER} [data-part=no-steer]`).scrollIntoViewIfNeeded();
        await p.page.screenshot({ path: `${dir}/composer-ready-no-steer-${size}.png`, fullPage: false });
      } finally {
        await p.close();
        await fx.close();
      }
    }
    // The launcher: the same tray, in the same prompt body.
    {
      const { fx } = await launcherSite();
      mixed(fx);
      const p = await openLauncher(browser, fx, { viewport });
      try {
        await chooseAll(p.page);
        await typePrompt(p.page, 'Synthetic prompt: reply ok');
        await addFiles(p.page, LAUNCHER, FILES);
        await settledMixed(p.page, LAUNCHER);
        await demo(p.page);
        await p.page.locator(`${LAUNCHER} [data-part=tray]`).scrollIntoViewIfNeeded();
        await p.page.screenshot({ path: `${dir}/launcher-tray-states-${size}.png`, fullPage: false });
      } finally {
        await p.close();
        await fx.close();
      }
    }
  }
  console.log(`attachment screens in ${dir}`);
} finally {
  await browser.close();
}
