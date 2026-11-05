

import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';

import { DESKTOP, launchBrowser, PHONE, readDetail } from '../support/browser.mjs';
import { composerSite, openThread } from '../support/composer.mjs';

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(() => browser.close());

const placement = (page) =>
  page.evaluate(() => {
    const c = document.querySelector('[data-part=composer]');
    return {
      dialogs: document.querySelectorAll('dialog').length,
      composers: document.querySelectorAll('[data-part=composer]').length,
      inDetail: !!c?.closest('dialog[data-part=detail]'),
      after: c?.previousElementSibling?.getAttribute('data-field') ?? null,
      textareas: document.querySelectorAll('textarea').length,
    };
  });

for (const [name, viewport, modal] of [
  ['phone: in the bottom sheet', PHONE, true],
  ['desktop: in the side panel', DESKTOP, false],
]) {
  test(`${name}, directly under the final output`, async () => {
    for (const finalOutput of [null, { known: { kind: 'no_final_output_yet' } }]) {
      const { fx } = await composerSite({ finalOutput });
      const p = await openThread(browser, fx, 1, { viewport });
      try {
        assert.equal((await readDetail(p.page)).modal, modal);
        const where = await placement(p.page);
        assert.deepEqual(where, { dialogs: 1, composers: 1, inDetail: true, after: 'final-output', textareas: 1 });
        if (finalOutput) assert.equal((await readDetail(p.page)).finalOutput, 'no final output yet');
      } finally {
        await p.close();
        await fx.close();
      }
    }
  });
}
