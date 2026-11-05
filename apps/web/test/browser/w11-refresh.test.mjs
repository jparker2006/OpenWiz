// W11: the composer coexists with 0.0.2's auto-refresh. A refresh never
// clears the draft, moves the focus or the caret, changes a shown outcome,
// or sends anything.

import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';

import { TIMING } from '../../src/timing.ts';
import { launchBrowser, PHONE, waitFor } from '../support/browser.mjs';
import { DROP, json } from '../support/fake-api.mjs';
import { button, composerSite, id, openThread, outcome, posts, readComposer, TEXT, type } from '../support/composer.mjs';
import { DELIVERIES, inventory, outcome as result, report, session } from '../support/synthetic.ts';

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(() => browser.close());

/** Runs `n` list and detail refreshes, each with changed data. */
async function refreshes(p, fx, state, n = 6) {
  for (let i = 0; i < n; i++) {
    const reads = fx.api.reads('detail').length + fx.api.reads('list').length;
    state.sessions = state.sessions.map((s, k) => ({ ...s, status: { known: ['idle', 'active', 'error'][(i + k) % 3] }, age_ms: { known: (i + 1) * 1000 } }));
    fx.api.setList(() => json(200, inventory(state.sessions)));
    await p.page.clock.runFor(TIMING.detailIntervalMs);
    await waitFor(() => fx.api.reads('detail').length + fx.api.reads('list').length > reads, { what: 'a refresh' });
  }
}

for (const viewport of [null, PHONE]) {
  test(`refreshes keep the draft, the focus and the caret, and send nothing${viewport ? ' (phone sheet)' : ''}`, async () => {
    const { fx, state } = await composerSite({ kinds: { [id(1)]: 'busy' } });
    const p = await openThread(browser, fx, 1, viewport ? { viewport } : {});
    try {
      const box = p.page.locator('[data-part=composer] textarea');
      await box.click();
      await p.page.keyboard.type(TEXT);
      await box.evaluate((t) => t.setSelectionRange(10, 14));
      await refreshes(p, fx, state);
      const c = await readComposer(p.page);
      assert.equal(c.draft, TEXT);
      assert.equal(c.focused, true, 'the focus stays in the composer');
      assert.deepEqual(c.selection, [10, 14], 'the caret and selection stay');
      assert.equal(posts(fx).length, 0, 'no refresh sends');
      assert.ok(fx.api.reads('detail').length >= 6 && fx.api.log.filter((e) => e.kind === 'preview').length >= 6, 'the previews followed the refreshes');
      await p.page.keyboard.type('!');
      assert.equal((await readComposer(p.page)).draft, `${TEXT.slice(0, 10)}!${TEXT.slice(14)}`, 'typing goes on where it was');
    } finally {
      await p.close();
      await fx.close();
    }
  });
}

test('refreshes never change a shown outcome, and never retry an uncertain send', async () => {
  const { fx, state } = await composerSite();
  let n = 0;
  fx.api.setSend((tid, b) => (n++ === 0 ? json(200, report(tid, b.op_id, b.mode, b.text, result(DELIVERIES.failed))) : { after: DROP, report: report(tid, b.op_id, b.mode, b.text, result(DELIVERIES.sent)) }));
  const p = await openThread(browser, fx, 1);
  try {
    await type(p.page, TEXT);
    await button(p.page, 'Send').click();
    const failed = await outcome(p.page, 'failed');
    await refreshes(p, fx, state, 3);
    assert.deepEqual(await readComposer(p.page).then((c) => [c.outcome, c.word, c.lines, c.draft]), [failed.outcome, failed.word, failed.lines, TEXT]);
    await button(p.page, 'Send').click();
    const lost = await outcome(p.page, 'uncertain');
    await refreshes(p, fx, state, 4);
    const c = await readComposer(p.page);
    assert.deepEqual([c.outcome, c.lines, c.draft, c.readOnly], [lost.outcome, lost.lines, TEXT, true]);
    assert.equal(posts(fx).length, 2, 'two presses, two requests: no refresh sent or retried');
    assert.equal(fx.api.log.filter((e) => e.kind === 'lookup').length, 0, 'and no refresh checks on its own');
  } finally {
    await p.close();
    await fx.close();
  }
});

test('a send in flight is untouched by refreshes, and its answer still lands', async () => {
  const { fx, state } = await composerSite();
  fx.api.hold('send');
  const p = await openThread(browser, fx, 1);
  try {
    await type(p.page, TEXT);
    await button(p.page, 'Send').click();
    await waitFor(() => fx.api.held('send') === 1, { what: 'the held send' });
    state.sessions = [session(1, { status: { known: 'active' } }), session(2), session(3)];
    // Two detail refreshes: 30 s, inside the composer's 45 s wait.
    await refreshes(p, fx, state, 2);
    assert.equal((await readComposer(p.page)).outcome, 'moving');
    fx.api.release('send');
    await outcome(p.page, 'sent');
    assert.equal(posts(fx).length, 1);
  } finally {
    await p.close();
    await fx.close();
  }
});
