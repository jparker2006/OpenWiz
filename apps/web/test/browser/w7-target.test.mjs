// W7: the right target. A send always goes to the open thread, from the
// path only. Drafts are kept per thread; switching threads never sends to
// another thread; a send in flight lands on its own thread. A thread that
// leaves the list can't be sent to, and says why.

import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';

import { launchBrowser, readDetail, waitFor } from '../support/browser.mjs';
import { json } from '../support/fake-api.mjs';
import { button, composerSite, id, openThread, outcome, posts, readComposer, select, TEXT, type } from '../support/composer.mjs';
import { inventory, session } from '../support/synthetic.ts';

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(() => browser.close());

test('the send goes to the open thread only, from the path; each thread keeps its own draft', async () => {
  const { fx } = await composerSite();
  const p = await openThread(browser, fx, 1);
  try {
    await type(p.page, 'Synthetic draft for one');
    await select(p.page, 2);
    assert.equal((await readComposer(p.page)).draft, '', 'thread 2 has its own, empty draft');
    await type(p.page, TEXT);
    await select(p.page, 1);
    assert.equal((await readComposer(p.page)).draft, 'Synthetic draft for one', 'thread 1 kept its draft');
    await select(p.page, 2);
    assert.equal((await readComposer(p.page)).draft, TEXT);
    await button(p.page, 'Send').click();
    await outcome(p.page, 'sent');
    assert.deepEqual(
      fx.api.calls.map((c) => c.id),
      [id(2)],
      'one call, to the open thread',
    );
    const sends = fx.api.log.filter((e) => e.kind === 'send');
    assert.deepEqual(
      sends.map((e) => e.path),
      [`/api/v1/sessions/${id(2)}/messages`],
    );
    assert.equal('target' in posts(fx)[0], false, 'the body carries no target');
    await select(p.page, 1);
    assert.equal((await readComposer(p.page)).draft, 'Synthetic draft for one', 'thread 1 is untouched');
    assert.equal((await readComposer(p.page)).outcome, 'none');
  } finally {
    await p.close();
    await fx.close();
  }
});

test('switching threads while a send is out: it lands on its own thread, and nothing goes to the other', async () => {
  const { fx } = await composerSite();
  fx.api.hold('send');
  const p = await openThread(browser, fx, 1);
  try {
    await type(p.page, TEXT);
    await button(p.page, 'Send').click();
    await outcome(p.page, 'moving');
    await waitFor(() => fx.api.held('send') === 1, { what: 'the held send' });
    await select(p.page, 3);
    let c = await readComposer(p.page);
    assert.equal(c.outcome, 'none', 'thread 3 shows nothing of thread 1’s send');
    assert.equal(c.draft, '');
    fx.api.release('send');
    await waitFor(() => fx.api.log.some((e) => e.kind === 'send' && e.completed), { what: 'the answer' });
    c = await readComposer(p.page);
    assert.equal(c.outcome, 'none', 'the answer for thread 1 never shows on thread 3');
    await select(p.page, 1);
    await outcome(p.page, 'sent');
    assert.deepEqual(
      fx.api.calls.map((x) => x.id),
      [id(1)],
    );
  } finally {
    await p.close();
    await fx.close();
  }
});

test('a thread that leaves the list can’t be sent to, and the composer says why', async () => {
  const { fx, state } = await composerSite();
  const p = await openThread(browser, fx, 2);
  try {
    await type(p.page, TEXT);
    // Thread 2 leaves the list on the next refresh.
    state.sessions = [session(1), session(3)];
    fx.api.setList(() => json(200, inventory(state.sessions)));
    await p.page.clock.runFor(10_000);
    await waitFor(async () => (await readDetail(p.page))?.noLongerListed, { what: 'no longer listed' });
    const c = await waitFor(async () => {
      const r = await readComposer(p.page);
      return r.off ? r : null;
    });
    assert.match(c.off, /can’t send: this thread is no longer listed on this page/);
    assert.ok(c.actions.every((a) => a.disabled), 'every send button is disabled');
    assert.equal(c.draft, TEXT, 'the draft stays');
    await p.page.locator('[data-part=composer] textarea').press('Control+Enter');
    await p.page.clock.runFor(1_000);
    assert.equal(posts(fx).length, 0, 'nothing was sent');
  } finally {
    await p.close();
    await fx.close();
  }
});
