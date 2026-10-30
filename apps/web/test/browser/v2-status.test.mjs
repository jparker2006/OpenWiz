// V2: status without colour. Each status can be told apart in greyscale by its
// word and its shape, and the contrast passes WCAG AA.

import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';

import { DESKTOP, launchBrowser, openPage, PHONE, readView, waitFor } from '../support/browser.mjs';
import { json } from '../support/fake-api.mjs';
import { axe, fakeSite } from '../support/harness.mjs';
import { inkMask, decodePng, maskDifference } from '../support/png.mjs';
import { inventory, statusSessions } from '../support/synthetic.ts';

/** Two markers are told apart when their colourless masks differ in at least this many pixels. */
export const DISTINCT_PIXELS = 20;

let browser;
let fx;
before(async () => {
  browser = await launchBrowser();
  fx = await fakeSite();
  fx.api.setList(() => json(200, inventory(statusSessions())));
});
after(async () => {
  await fx.close();
  await browser.close();
});

async function rendered(viewport) {
  const p = await openPage(browser, fx.site.origin, { viewport });
  await waitFor(async () => (await readView(p.page)).groups.length > 0, { what: 'rows' });
  return p;
}

test('every status shows the core\'s word, a glyph and a tier shape', async () => {
  const p = await rendered(PHONE);
  try {
    const rows = (await readView(p.page)).groups.flatMap((g) => g.rows);
    assert.deepEqual(
      rows.map((r) => r.status),
      [
        { word: 'error', glyph: '[x]', tier: 't1' },
        { word: 'active', glyph: '[~]', tier: 't3' },
        { word: 'idle', glyph: '[-]', tier: 't4' },
        { word: 'syn-other', glyph: '[?]', tier: 't5' },
        { word: 'unknown (missing field status)', glyph: '[?]', tier: 't5' },
      ],
    );
    // The glyph is decoration; the word is what a screen reader hears.
    const hidden = await p.page.$$eval('[data-part=status] .glyph', (els) => els.map((e) => e.getAttribute('aria-hidden')));
    assert.ok(hidden.every((h) => h === 'true'));
  } finally {
    await p.close();
  }
});

/**
 * Greyscale masks of each tier's marker, with every word replaced by the same
 * text, so only the glyph and the shape are left to tell them apart.
 */
async function tierMasks(page) {
  await page.evaluate(() => {
    document.documentElement.style.filter = 'grayscale(1)';
    for (const w of document.querySelectorAll('[data-part=status] .word')) w.textContent = 'status';
  });
  const masks = {};
  for (const tier of ['t1', 't3', 't4', 't5']) {
    const box = await page.locator(`[data-part=status][data-tier=${tier}]`).first().boundingBox();
    assert.ok(box, `a ${tier} marker is shown`);
    const png = await page.screenshot({ clip: { x: box.x - 4, y: box.y - 4, width: box.width + 8, height: box.height + 8 } });
    masks[tier] = inkMask(decodePng(png));
  }
  return masks;
}

test('in greyscale, every tier differs from every other by glyph and shape alone', async () => {
  for (const viewport of [PHONE, DESKTOP]) {
    const p = await rendered(viewport);
    try {
      const masks = await tierMasks(p.page);
      const tiers = Object.keys(masks);
      const diffs = {};
      for (let i = 0; i < tiers.length; i++) {
        for (let j = i + 1; j < tiers.length; j++) {
          const d = maskDifference(masks[tiers[i]], masks[tiers[j]]);
          diffs[`${tiers[i]}-${tiers[j]}`] = d;
          assert.ok(d >= DISTINCT_PIXELS, `${tiers[i]} and ${tiers[j]} look the same in greyscale (${d} px differ)`);
        }
      }
      console.log(`greyscale mask differences at ${viewport.width}px:`, JSON.stringify(diffs));
    } finally {
      await p.close();
    }
  }
});

test('contrast passes WCAG AA (axe color-contrast)', async () => {
  for (const viewport of [PHONE, DESKTOP]) {
    const p = await rendered(viewport);
    try {
      assert.deepEqual(await axe(p.page, ['wcag2aa']), []);
    } finally {
      await p.close();
    }
  }
});
