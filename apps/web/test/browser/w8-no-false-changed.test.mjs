

import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';

import { TIMING } from '../../src/timing.ts';
import { launchBrowser, pageIdle, readView, waitFor } from '../support/browser.mjs';
import { composerSite, id, openThread, posts, readComposer, select, TEXT, type } from '../support/composer.mjs';

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(() => browser.close());

const brief = (c) => c.actions.map((a) => [a.label, a.mode, a.effect]);

const BUTTONS = {
  idle: [['Send', 'queue', 'starts a turn; may wake it']],
  busy: [
    ['Queue', 'queue', 'queues behind the current turn'],
    ['Steer', 'steer', 'asks to steer the current turn'],
  ],
};

/**
 * From now on, records in the page every time the composer's `· changed`
 * mark is drawn, with its composer's target: a mark drawn even for a moment
 * is caught, not only one a poll happens to land on.
 */
async function watchMark(page) {
  await page.evaluate(() => {
    window.__marks = [];
    const look = () => {
      const m = document.querySelector('[data-part=composer] [data-part=effect-changed]');
      if (m && window.__marks.at(-1) !== m) window.__marks.push(m);
    };
    look();
    new MutationObserver(look).observe(document.body, { childList: true, subtree: true, characterData: true });
  });
}

/** The marks drawn since `watchMark`, by their composer's target. */
const marks = (page) => page.evaluate(() => window.__marks.map((m) => m.closest('[data-part=composer]')?.dataset.target ?? null));

/**
 * Waits for two later renders of the composer: any effect of a render before
 * them has run, and anything it set is drawn. The mark is set in an effect
 * after the render it judges, and React runs a render's effects before
 * it starts the next one, so the first later render follows the effect and
 * the second draws what it set. The age line is the composer's own render
 * output; a one-second step of the page's clock always moves it.
 */
async function settle(page) {
  for (let i = 0; i < 2; i++) {
    const was = (await readComposer(page)).age;
    assert.match(was ?? '', /^effect read \d+s ago/, 'the read’s age is shown');
    await page.clock.runFor(1_000);
    await waitFor(async () => {
      const now = (await readComposer(page)).age;
      return now !== was ? now : null;
    }, { what: `the age to move from “${was}”: a later render` });
  }
}

/** How many previews of thread `n` the fake API has been asked for. */
const previewsOf = (fx, n) => fx.api.reads('preview').filter((e) => e.path.split('/')[4] === id(n)).length;

for (const kind of ['idle', 'busy']) {
  const other = kind === 'idle' ? 'busy' : 'idle';

  test(`a saved draft shows no “· changed” after switching to another thread and back, or after a refresh reads the same state; a real change is still marked`, async () => {
    const { fx, state } = await composerSite({ kinds: { [id(1)]: kind, [id(2)]: 'idle' } });
    const p = await openThread(browser, fx, 1, { stampReads: true });
    try {
      await type(p.page, TEXT);
      let c = await readComposer(p.page);
      assert.deepEqual(brief(c), BUTTONS[kind], 'before switching');
      assert.equal(c.changed, false, 'no mark before switching');

      await select(p.page, 2);
      assert.equal((await readComposer(p.page)).draft, '', 'thread 2 has no draft');
      await watchMark(p.page);
      await select(p.page, 1);
      await settle(p.page);
      c = await readComposer(p.page);
      assert.equal(c.draft, TEXT, 'the saved draft is back');
      assert.deepEqual(brief(c), BUTTONS[kind], 'the same buttons and effects as before switching');
      assert.deepEqual(await marks(p.page), [], 'switching back: the state did not change, so no mark is drawn');
      assert.equal(c.changed, false);

      // A refresh reads the same state again: still no mark.
      const read = previewsOf(fx, 1);
      await p.page.clock.runFor(TIMING.detailIntervalMs);
      await waitFor(() => previewsOf(fx, 1) > read, { what: 'thread 1’s preview read again' });
      await waitFor(() => pageIdle(p.page), { what: 'the page read every answer' });
      await settle(p.page);
      c = await readComposer(p.page);
      assert.deepEqual(brief(c), BUTTONS[kind], 'the refresh read the same state');
      assert.deepEqual(await marks(p.page), [], 'the same state read again: no mark');

      // A real change after switching back is still marked.
      state.kinds[id(1)] = other;
      await p.page.clock.runFor(TIMING.detailIntervalMs);
      c = await waitFor(async () => {
        const r = await readComposer(p.page);
        return r.changed && r.ready ? r : null;
      }, { what: 'the change mark after a real change' });
      assert.deepEqual(brief(c), BUTTONS[other], 'the mark is on the new buttons');
      assert.equal(c.draft, TEXT, 'the draft stays');
      assert.deepEqual(await marks(p.page), [id(1)], 'one mark, for the real change');
      assert.equal(posts(fx).length, 0, 'nothing is sent');
    } finally {
      await p.close();
      await fx.close();
    }
  });
}

test('no “· changed” while the switched-back composer’s preview is out, nor when it answers with the same state', async () => {
  const { fx } = await composerSite({ kinds: { [id(1)]: 'idle', [id(2)]: 'idle' } });
  const p = await openThread(browser, fx, 1);
  try {
    await type(p.page, TEXT);
    await select(p.page, 2);
    await watchMark(p.page);
    fx.api.hold('preview');
    const held = fx.api.held('preview');
    await p.page.click(`main [data-session-id="${id(1)}"]`);
    await waitFor(async () => (await readComposer(p.page))?.target === id(1), { what: 'thread 1’s composer' });
    await waitFor(() => fx.api.held('preview') > held, { what: 'the held preview' });
    let c = await readComposer(p.page);
    assert.equal(c.preview, 'loading', 'its preview is out');
    assert.equal(c.draft, TEXT, 'the saved draft is back');
    assert.equal(c.changed, false, 'no mark while the preview is out');
    fx.api.hold('preview', false);
    fx.api.release('preview');
    await waitFor(async () => (await readComposer(p.page)).preview === 'ready', { what: 'the preview’s answer' });
    await settle(p.page);
    c = await readComposer(p.page);
    assert.deepEqual(brief(c), BUTTONS.idle);
    assert.deepEqual(await marks(p.page), [], 'the first answer is the baseline, not a change from the placeholder');
    assert.equal(posts(fx).length, 0, 'nothing is sent');
  } finally {
    await p.close();
    await fx.close();
  }
});

test('a draft restored after a reload shows no “· changed”', async () => {
  const { fx } = await composerSite({ kinds: { [id(1)]: 'idle' } });
  const p = await openThread(browser, fx, 1);
  try {
    await type(p.page, TEXT);
    await p.page.reload();
    await waitFor(async () => (await readView(p.page)).groups.length > 0, { what: 'rows after the reload' });
    await watchMark(p.page);
    await select(p.page, 1);
    await settle(p.page);
    const c = await readComposer(p.page);
    assert.equal(c.draft, TEXT, 'the draft is restored from this tab’s storage');
    assert.deepEqual(brief(c), BUTTONS.idle);
    assert.deepEqual(await marks(p.page), [], 'a restored draft over the same state: no mark');
    assert.equal(posts(fx).length, 0, 'nothing is sent');
  } finally {
    await p.close();
    await fx.close();
  }
});
