// W8: the action and its effects come before pressing, from the core's
// preview: idle is Send ("starts a turn; may wake it"); a running turn is
// Queue (the default) and Steer (explicit); a paused queue says the send
// resumes it, with how many may follow; anything else in the core's words.
// A state change while composing changes the label and effect visibly. The
// request carries the pressed button's mode, and nothing changes it after.

import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';

import { TIMING } from '../../src/timing.ts';
import { launchBrowser, waitFor } from '../support/browser.mjs';
import { apiError, json } from '../support/fake-api.mjs';
import { button, composerSite, id, openThread, outcome, posts, readComposer, select, TEXT, type } from '../support/composer.mjs';
import { preview } from '../support/synthetic.ts';

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(() => browser.close());

const brief = (c) => c.actions.map((a) => [a.label, a.mode, a.effect]);

test('each state shows its buttons and the core’s effect line, with the read’s age, before pressing', async () => {
  const { fx } = await composerSite({ count: 6, kinds: { [id(1)]: 'idle', [id(2)]: 'busy', [id(3)]: 'paused', [id(4)]: 'interaction', [id(5)]: 'unknown', [id(6)]: 'refused' } });
  const p = await openThread(browser, fx, 1);
  try {
    const expect = {
      1: [['Send', 'queue', 'starts a turn; may wake it']],
      2: [
        ['Queue', 'queue', 'queues behind the current turn'],
        ['Steer', 'steer', 'asks to steer the current turn'],
      ],
      3: [['Send and resume queue', 'queue', 'starts a turn; may wake it']],
      4: [['Queue', 'queue', 'waits for the pending interaction to be answered']],
      5: [['Queue', 'queue', 'unknown (its status: the status syn-starting is not one OpenWiz knows)']],
      6: [['Send', 'queue', 'can’t send: the target is archived, and archived targets are refused']],
    };
    for (const [n, want] of Object.entries(expect)) {
      if (n !== '1') await select(p.page, Number(n));
      const c = await readComposer(p.page);
      assert.deepEqual(brief(c), want, `thread ${n}`);
      assert.equal(c.paused, n === '3' ? 'resumes the paused queue; 2 queued messages may run after this one' : null, `thread ${n}: the paused-queue line`);
      if (n === '6') assert.ok(c.actions.every((a) => a.disabled), 'a refused target can’t be sent to');
      else {
        assert.match(c.age, /^effect read \d+s ago$/, `thread ${n}: the read's age`);
        await type(p.page, TEXT);
        assert.ok(c.actions.length > 0);
        assert.ok((await readComposer(p.page)).actions.every((a) => !a.disabled), `thread ${n}: the buttons are enabled with text`);
        await type(p.page, '');
      }
    }
    // The previews asked for: queue for each, and steer only for the busy thread.
    const previews = fx.api.log.filter((e) => e.kind === 'preview').map((e) => `${e.path.split('/')[4]}${e.search}`);
    assert.ok(previews.includes(`${id(2)}?mode=steer`));
    assert.ok(!previews.some((x) => x.endsWith('mode=steer') && !x.startsWith(id(2))), 'steer is previewed only for a running turn');
    assert.equal(posts(fx).length, 0, 'reading the effects sends nothing');
  } finally {
    await p.close();
    await fx.close();
  }
});

test('the pressed button’s mode is the one sent; the paused queue is resumed only by its own button', async () => {
  const { fx } = await composerSite({ kinds: { [id(1)]: 'busy', [id(2)]: 'paused' } });
  const p = await openThread(browser, fx, 1);
  try {
    await type(p.page, TEXT);
    await button(p.page, 'Steer').click();
    await outcome(p.page, 'sent');
    await type(p.page, `${TEXT} again`);
    await button(p.page, 'Queue').click();
    await waitFor(() => posts(fx).length === 2 && fx.api.calls.length === 2, { what: 'the second send' });
    await select(p.page, 2);
    await type(p.page, TEXT);
    await button(p.page, 'Send and resume queue').click();
    await waitFor(() => fx.api.calls.length === 3, { what: 'the third send' });
    assert.deepEqual(
      posts(fx).map((b) => [b.mode, b.resume_paused_queue, b.accept_possible_duplicate]),
      [
        ['steer', false, false],
        ['queue', false, false],
        ['queue', true, false],
      ],
    );
  } finally {
    await p.close();
    await fx.close();
  }
});

test('a state change while composing changes the label and effect visibly; the draft stays; the press sends what was pressed', async () => {
  const { fx, state } = await composerSite({ kinds: { [id(1)]: 'idle' } });
  const p = await openThread(browser, fx, 1);
  try {
    await type(p.page, TEXT);
    let c = await readComposer(p.page);
    assert.deepEqual(brief(c), [['Send', 'queue', 'starts a turn; may wake it']]);
    assert.equal(c.changed, false);
    // The thread starts a turn; the next detail refresh re-reads the preview.
    state.kinds[id(1)] = 'busy';
    await p.page.clock.runFor(TIMING.detailIntervalMs);
    c = await waitFor(async () => {
      const r = await readComposer(p.page);
      return r.actions.length === 2 && r.ready ? r : null;
    }, { what: 'the busy buttons' });
    assert.deepEqual(brief(c), [
      ['Queue', 'queue', 'queues behind the current turn'],
      ['Steer', 'steer', 'asks to steer the current turn'],
    ]);

    c = await waitFor(async () => {
      const r = await readComposer(p.page);
      return r.changed ? r : null;
    }, { what: 'the change mark: the change is marked, not only redrawn' });
    assert.deepEqual(brief(c), [
      ['Queue', 'queue', 'queues behind the current turn'],
      ['Steer', 'steer', 'asks to steer the current turn'],
    ], 'the mark is on the busy buttons');
    assert.equal(c.draft, TEXT, 'the draft stays');
    // Pressed, then the thread goes idle while the send is out: the request keeps Steer.
    fx.api.hold('send');
    await button(p.page, 'Steer').click();
    await waitFor(() => fx.api.held('send') === 1, { what: 'the held send' });
    state.kinds[id(1)] = 'idle';
    fx.api.setPreview((tid, mode) => json(200, preview(tid, state.kinds[tid] ?? 'idle', mode)));
    await p.page.clock.runFor(TIMING.detailIntervalMs);
    fx.api.release('send');
    c = await outcome(p.page, 'sent');
    assert.deepEqual(
      posts(fx).map((b) => b.mode),
      ['steer'],
    );
    assert.match(c.lines[0], /^Steer requested · /, 'the outcome names the requested mode');
  } finally {
    await p.close();
    await fx.close();
  }
});

test('no preview, no send: a failed preview disables sending with a reason; an unknown effect does not', async () => {
  const { fx } = await composerSite({ kinds: { [id(2)]: 'unknown' } });
  fx.api.setPreview((tid, mode) => (tid === id(1) ? apiError(503, 'busy', 'no core run slot within its wait', { 'Retry-After': '5' }) : json(200, preview(tid, 'unknown', mode))));
  const p = await openThread(browser, fx, 1);
  try {
    await type(p.page, TEXT);
    let c = await readComposer(p.page);
    assert.ok(c.actions.every((a) => a.disabled), 'no button without the core’s preview');
    assert.match(c.actions[0].effect, /^can’t send: what a send would do couldn’t be read \(busy: no core run slot within its wait\)$/);
    assert.equal(c.age, null, 'no age without a read');
    await p.page.locator('[data-part=composer] textarea').press('Control+Enter');
    await p.page.clock.runFor(1_000);
    assert.equal(posts(fx).length, 0);
    await select(p.page, 2);
    await type(p.page, TEXT);
    c = await readComposer(p.page);
    assert.deepEqual(brief(c), [['Queue', 'queue', 'unknown (its status: the status syn-starting is not one OpenWiz knows)']]);
    assert.ok(c.actions.every((a) => !a.disabled), 'a valid preview whose effect is unknown still lets the owner send');
  } finally {
    await p.close();
    await fx.close();
  }
});

test('5: the composer is not ready until its preview has answered; the helper waits for it', async () => {
  const { fx } = await composerSite({ kinds: { [id(2)]: 'busy' } });
  const p = await openThread(browser, fx, 1);
  try {
    fx.api.hold('preview');
    await p.page.click(`main [data-session-id="${id(2)}"]`);
    await waitFor(async () => (await readComposer(p.page))?.target === id(2), { what: 'thread 2’s composer' });
    await waitFor(() => fx.api.held('preview') > 0, { what: 'the held preview' });
    const c = await readComposer(p.page);
    assert.equal(c.ready, false, 'not ready while the preview is out');
    assert.equal(await p.page.locator('[data-part=composer]').getAttribute('data-preview'), 'loading');
    fx.api.hold('preview', false);
    fx.api.release('preview');
    await select(p.page, 2);
    const done = await readComposer(p.page);
    assert.deepEqual(brief(done), [
      ['Queue', 'queue', 'queues behind the current turn'],
      ['Steer', 'steer', 'asks to steer the current turn'],
    ]);
  } finally {
    await p.close();
    await fx.close();
  }
});
