

import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';

import { DESKTOP, launchBrowser, openPage, PHONE, readDetail, readView, waitFor } from '../support/browser.mjs';
import { button, readComposer, select as selectThread, TEXT, type } from '../support/composer.mjs';
import { axe } from '../support/harness.mjs';
import { startFixtureServer } from '../support/rust.mjs';
import { startSite } from '../support/serve.mjs';

const HOST = 'openwiz.example.test';

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(() => browser.close());

async function realSite(responses = null) {
  const server = await startFixtureServer('send', { allowedHost: HOST, responses });
  const site = await startSite({ port: server.port, allowedHost: HOST });
  return {
    server,
    site,
    tells: () => server.invocations().filter((a) => a[0] === 'thread' && a[1] === 'tell'),
    sends: () => site.log.filter((e) => e.method === 'POST'),
    close: async () => {
      await site.close();
      await server.stop();
    },
  };
}

/** Opens the real site and thread `id` (a `thr_syn_` fixture id), and waits for its composer's preview. */
async function open(fx, id, viewport = DESKTOP) {
  const p = await openPage(browser, fx.site.origin, { viewport });
  await waitFor(async () => (await readView(p.page)).groups.length > 0, { what: 'rows' });
  await choose(p.page, id);
  return p;
}

async function choose(page, id) {
  if ((await readDetail(page))?.modal) {
    await page.click('[data-part=detail] .sheet-x');
    await waitFor(async () => (await readDetail(page)) === null, { what: 'the sheet to close' });
  }
  await page.click(`main [data-session-id="${id}"]`);
  await waitFor(async () => {
    const c = await readComposer(page);
    return c?.target === id && c.ready;
  }, { what: `the composer of ${id}` });
}

const settled = (page, n) => waitFor(async () => {
  const c = await readComposer(page);
  return c.rows.length >= n && !['moving'].includes(c.outcome) ? c : null;
}, { what: `${n} answered attempt(s)` });

test('real server: W7, W8, W9: the open thread, the core’s preview, and the core’s outcomes', async () => {
  const fx = await realSite();
  const p = await open(fx, 'thr_syn_0001');
  try {
    let c = await readComposer(p.page);
    assert.deepEqual(c.actions.map((a) => [a.label, a.effect]), [['Send', 'starts a turn; may wake it']], 'idle, from the core');
    assert.match(c.age, /^effect read \d+s ago$/);
    await type(p.page, TEXT);
    await button(p.page, 'Send').click();
    c = await settled(p.page, 1);
    assert.deepEqual([c.outcome, c.word], ['sent', 'sent']);
    assert.match(c.lines[0], /^Queue requested · sent: /);
    assert.deepEqual(fx.tells().map((a) => a.slice(0, 5)), [['thread', 'tell', 'thr_syn_0001', '--mode', 'queue']], 'one call, to the open thread');
    assert.equal(c.draft, '');
    // A running turn: Queue and Steer, each with the core's effect; Steer queues with no reported reason; Queue waits for the busy thread.
    await choose(p.page, 'thr_syn_0002');
    c = await readComposer(p.page);
    assert.deepEqual(c.actions.map((a) => [a.label, a.mode, a.effect]), [
      ['Queue', 'queue', 'queues behind the current turn'],
      ['Steer', 'steer', 'asks to steer the current turn'],
    ]);
    await type(p.page, TEXT);
    await button(p.page, 'Steer').click();
    c = await settled(p.page, 1);
    assert.deepEqual([c.outcome, c.word], ['queued', 'queued']);
    assert.match(c.lines[0], /^Steer requested · queued: waiting reason not reported/);
    assert.equal(c.draft, '', 'confirmed queued Steer clears its draft');
    const steerRows = c.rows;
    // A confirmed delivery ends that draft; the next draft has its own history.
    await type(p.page, `${TEXT} queue`);
    await button(p.page, 'Queue').click();
    c = await settled(p.page, 1);
    const completedRows = [...steerRows, ...c.rows];
    assert.deepEqual(completedRows.map((r) => r.word), ['queued', 'queued'], 'every completed draft, in order');
    assert.match(c.lines[0], /^Queue requested · queued: thread busy/);
    assert.deepEqual(completedRows.map((r) => r.mode), ['steer', 'queue']);
    assert.equal(c.draft, '', 'confirmed queued acceptance clears its draft');
    // A paused queue: only its own button resumes it.
    await choose(p.page, 'thr_syn_0011');
    c = await readComposer(p.page);
    assert.deepEqual(c.actions.map((a) => a.label), ['Send and resume queue']);
    assert.equal(c.paused, 'resumes the paused queue; 2 queued messages may run after this one');
    await type(p.page, TEXT);
    await button(p.page, 'Send and resume queue').click();
    c = await settled(p.page, 1);
    assert.equal(c.word, 'sent');
    // A provider failure: the text stays.
    await choose(p.page, 'thr_syn_0005');
    await type(p.page, TEXT);
    await button(p.page, 'Send').click();
    c = await settled(p.page, 1);
    assert.equal(c.word, 'failed');
    assert.equal(c.draft, TEXT);
    assert.equal(fx.sends().length, 5);
    assert.equal(fx.tells().length, 5, 'one call per press');
    assert.deepEqual(await p.page.evaluate(() => Object.keys(sessionStorage).sort()), ['openwiz.composer.thr_syn_0005'], 'confirmed queued drafts clear; the failed draft stays');
  } finally {
    await p.close();
    await fx.close();
  }
});

test('real server: W10: a double click, and a tab duplicated before the send, make one call', async () => {
  const fx = await realSite();
  const p = await open(fx, 'thr_syn_0001');
  try {
    await type(p.page, TEXT);
    const storage = await p.page.evaluate(() => JSON.stringify(Object.entries(sessionStorage)));
    const second = await p.context.newPage();
    await second.addInitScript((entries) => {
      if (sessionStorage.length === 0) for (const [k, v] of JSON.parse(entries)) sessionStorage.setItem(k, v);
    }, storage);
    await second.goto(fx.site.origin);
    await waitFor(async () => (await readView(second)).groups.length > 0, { what: 'rows in the copy' });
    await choose(second, 'thr_syn_0001');
    await button(p.page, 'Send').dblclick();
    await settled(p.page, 1);
    await button(second, 'Send').click();
    const c = await settled(second, 1);
    assert.equal(c.word, 'sent', 'the copy gets the recorded outcome');
    assert.equal(fx.sends().length, 2);
    assert.equal(fx.tells().length, 1, 'one call: the copy’s request was a replay');
  } finally {
    await p.close();
    await fx.close();
  }
});

test('real server: W11, W12, W13: refreshes, placement, keyboard and axe, phone and desktop', async () => {
  for (const viewport of [PHONE, DESKTOP]) {
    const fx = await realSite();
    const p = await open(fx, 'thr_syn_0012', viewport);
    try {
      const where = await p.page.evaluate(() => {
        const c = document.querySelector('[data-part=composer]');
        return { dialogs: document.querySelectorAll('dialog').length, inDetail: !!c?.closest('dialog[data-part=detail]'), after: c?.previousElementSibling?.getAttribute('data-field') };
      });
      assert.deepEqual(where, { dialogs: 1, inDetail: true, after: 'final-output' });
      const box = p.page.locator('[data-part=composer] textarea');
      await box.click();
      await p.page.keyboard.type(TEXT);
      const reads = fx.site.log.filter((e) => e.path.startsWith('/api/')).length;
      await waitFor(() => fx.site.log.filter((e) => e.path.startsWith('/api/')).length > reads + 2, { what: 'refreshes', timeout: 40_000 });
      let c = await readComposer(p.page);
      assert.equal(c.draft, TEXT);
      assert.equal(c.focused, true);
      assert.equal(fx.tells().length, 0, 'no refresh sends');
      assert.deepEqual(await axe(p.page), []);
      await p.page.keyboard.press('Control+Enter');
      c = await settled(p.page, 1);
      assert.match(c.lines[0], /^Queue requested · uncertain:/);
      assert.equal(c.word, 'uncertain');
      assert.equal(c.draft, TEXT, 'the unobserved receipt keeps the text');
      assert.equal(c.live, 'status');
      assert.equal(fx.tells().length, 1);
      assert.deepEqual(await axe(p.page), []);
    } finally {
      await p.close();
      await fx.close();
    }
  }
});

test('real server: lost acknowledgements: Try again replays, Check again reads the record, Forget it sends the next attempt', async () => {
  const fx = await realSite();
  const p = await open(fx, 'thr_syn_0001');
  try {
    // Try again: the same op id, so the server's core replays the recorded outcome.
    fx.site.dropPostAnswers(1);
    await type(p.page, TEXT);
    await button(p.page, 'Send').click();
    let c = await settled(p.page, 1);
    assert.deepEqual([c.outcome, c.word], ['uncertain', 'uncertain'], 'a lost answer is uncertain');
    assert.equal(fx.tells().length, 1, 'the provider was called');
    await button(p.page, 'Try again').click();
    c = await waitFor(async () => ((await readComposer(p.page)).outcome === 'sent' ? readComposer(p.page) : null), { what: 'the replay' });
    assert.equal(fx.tells().length, 1, 'the retry was a replay: still one call');
    // Check again: a lookup of the record, no call.
    fx.site.dropPostAnswers(1);
    await type(p.page, `${TEXT} two`);
    await button(p.page, 'Send').click();
    await waitFor(async () => (await readComposer(p.page)).outcome === 'uncertain', { what: 'the second lost answer' });
    await button(p.page, 'Check again').click();
    await waitFor(async () => (await readComposer(p.page)).outcome === 'sent', { what: 'the record read' });
    assert.equal(fx.tells().length, 2);
    // Forget it: the next send is a new attempt, never the forgotten op id.
    fx.site.dropPostAnswers(1);
    await type(p.page, `${TEXT} three`);
    await button(p.page, 'Send').click();
    await waitFor(async () => (await readComposer(p.page)).outcome === 'uncertain', { what: 'the third lost answer' });
    await button(p.page, 'Forget it').click();
    await button(p.page, 'Send').click();
    await waitFor(async () => (await readComposer(p.page)).outcome === 'sent', { what: 'the next attempt' });
    const ops = fx.site.log.filter((e) => e.method === 'POST').length;
    assert.equal(ops, 5);
    assert.equal(fx.tells().length, 4, 'the forgotten attempt and the next one are two sends: the owner chose it');
  } finally {
    await p.close();
    await fx.close();
  }
});

test('real server: Check again by keyboard keeps focus while it reads; when its row settles, focus goes to the message field', async () => {
  const fx = await realSite();
  const p = await open(fx, 'thr_syn_0001');
  try {
    fx.site.dropPostAnswers(1);
    await type(p.page, TEXT);
    await button(p.page, 'Send').click();
    await waitFor(async () => (await readComposer(p.page)).outcome === 'uncertain', { what: 'the lost answer' });
    await button(p.page, 'Check again').focus();
    await p.page.keyboard.press('Enter');
    await waitFor(async () => (await readComposer(p.page)).outcome === 'sent', { what: 'the record read' });
    assert.equal(await p.page.evaluate(() => document.activeElement?.getAttribute('data-part')), 'draft');
    await p.page.keyboard.press('Tab');
    assert.notEqual(await p.page.evaluate(() => document.activeElement?.tagName), 'DIALOG', 'the next Tab stays in the composer');
  } finally {
    await p.close();
    await fx.close();
  }
});

test('real server: holding Enter on Send again across the answer makes one more call, not two', async () => {
  // Synthetic fault: every queue tell to thr_syn_0001 answers non-JSON, which the core reports as uncertain.
  const fx = await realSite([{ argv: ['thread', 'tell', 'thr_syn_0001', '--mode', 'queue', '--json', '--'], any_text: true, stdout: 'synthetic: not json' }]);
  const p = await open(fx, 'thr_syn_0001');
  try {
    await type(p.page, TEXT);
    await button(p.page, 'Send').click();
    await waitFor(async () => (await readComposer(p.page)).outcome === 'uncertain', { what: 'A uncertain' });
    await button(p.page, 'Send again').focus();
    await p.page.keyboard.down('Enter');
    await waitFor(async () => {
      const c = await readComposer(p.page);
      return c.rows.length === 2 && c.outcome === 'uncertain';
    }, { what: 'B uncertain' });
    for (let i = 0; i < 8; i++) await p.page.keyboard.down('Enter');
    await waitFor(async () => (await readComposer(p.page)).ready, { what: 'settled' });
    await p.page.keyboard.up('Enter');
    assert.equal(fx.sends().length, 2);
    assert.equal(fx.tells().length, 2, 'the original call plus one resend');
  } finally {
    await p.close();
    await fx.close();
  }
});
