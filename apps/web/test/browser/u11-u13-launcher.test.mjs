// U11 to U13: what a refusal, an uncertain create and the page's
// refresh do to the launcher, on the fake API. Synthetic values only.
//
// - U11 failure and uncertainty keep everything: the prompt, settings and op
// id survive a reload; Refresh status makes no create; there is no retry
//, only the owner's explicit Start over, which says it may make a
// second thread. "Not offered" reads the options again, visibly.
// - U12 no double create: double clicks, a held Ctrl+Enter, Try again after no
// answer, a reload and a copied tab all send one op id: one provider call.
// - U13 refreshes never clear the draft or settings, move focus, change a
// shown outcome or create anything.

import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';

import { TIMING } from '../../src/timing.ts';
import { launchBrowser, waitFor } from '../support/browser.mjs';
import { apiError, DROP, HANG, json } from '../support/fake-api.mjs';
import { chooseAll, createPosts, launcherSite, openLauncher, pressNewThread, PROMPT, readLauncher, ready, settled, typePrompt } from '../support/launcher.mjs';
import * as nt from '../support/newthread-synthetic.ts';

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(() => browser.close());

// `force`: Start thread is `aria-disabled` while a create is out, and a press then must do nothing.
const press = (page) => page.click('[data-part=launcher] [data-part=start]', { force: true });
const creates = (fx) => fx.api.log.filter((e) => e.kind === 'create' && e.method === 'POST');

async function fillAndReady(page) {
  await chooseAll(page);
  await typePrompt(page, PROMPT);
  return ready(page);
}

test('U11: a refusal keeps the prompt and settings; "not offered" reads the options again, visibly', async () => {
  const { fx } = await launcherSite();
  fx.api.setCreate((project, b) => json(200, nt.report(project, b, nt.OUTCOMES.notOffered(['syn-codex-model-02']))));
  const p = await openLauncher(browser, fx);
  try {
    await fillAndReady(p.page);
    const optionsBefore = fx.api.log.filter((e) => e.kind === 'options').length;
    await press(p.page);
    const l = await settled(p.page, 'not-sent');
    assert.equal(l.word, 'refused');
    assert.match(l.lines[0], /refused: the model is not offered for this harness; nothing was created/, 'the core’s words');
    assert.equal(l.draft, PROMPT);
    assert.equal(l.readOnly, false);
    assert.deepEqual(l.picks.map((x) => x.value), ['codex', 'syn-codex-model-01', 'high']);
    await waitFor(() => fx.api.log.filter((e) => e.kind === 'options').length > optionsBefore, { what: 'the options read again' });
    // The next press is a new attempt: the refused op id is bound to its outcome.
    fx.api.setCreate((project, b) => json(200, nt.report(project, b, nt.OUTCOMES.created('thr_web_9001'))));
    await ready(p.page);
    await press(p.page);
    await settled(p.page, 'created');
    const ops = createPosts(fx).map((b) => b.op_id);
    assert.equal(ops.length, 2);
    assert.match(ops[0], /-c1$/);
    assert.match(ops[1], /-c2$/);
  } finally {
    await p.close();
    await fx.close();
  }
});

test('U11, U12: an uncertain create keeps everything across a reload; Refresh status never creates; only Start over makes a new attempt', async () => {
  const { fx } = await launcherSite();
  fx.api.setCreate((project, b) => ({ after: DROP, report: nt.report(project, b, nt.OUTCOMES.uncertain()) }));
  fx.api.setStatus((op, known) => json(200, nt.status(op, known?.report ?? null, null, 1)));
  const p = await openLauncher(browser, fx);
  try {
    await fillAndReady(p.page);
    await press(p.page);
    let l = await settled(p.page, 'uncertain');
    assert.equal(l.word, 'uncertain');
    assert.match(l.lines.join(' '), /the connection was lost before an answer/);
    assert.match(l.lines.join(' '), /may or may not have been created; the prompt, settings and op id stay/);
    assert.equal(l.readOnly, true, 'the prompt is fixed while it may have created a thread');
    assert.equal(l.start.label, 'Try again', 'no answer: Try again, the same op id');
    const op = createPosts(fx)[0].op_id;
    // Reload: everything is still there.
    await p.page.reload();
    await waitFor(async () => (await p.page.locator('main [data-new-thread]').count()) > 0, { what: 'rows' });
    await pressNewThread(p.page);
    l = await readLauncher(p.page);
    assert.equal(l.draft, PROMPT);
    assert.deepEqual(l.picks.map((x) => x.value), ['codex', 'syn-codex-model-01', 'high']);
    assert.equal(l.word, 'uncertain');
    // Refresh status: the record says uncertain; observations never settle it; nothing is created.
    await p.page.click('[data-part=launcher] [data-part=refresh-status]');
    l = await waitFor(async () => {
      const x = await readLauncher(p.page);
      return x.observations ? x : null;
    }, { what: 'the status read' });
    assert.match(l.observations, /^1 thread seen in this project since · observations only/);
    assert.deepEqual(l.unresolved, ['Refresh status', 'Start over']);
    assert.equal(l.start, null, 'uncertain: no Start, no Try again, no retry');
    assert.equal(creates(fx).length, 1, 'the status read created nothing');
    assert.equal(fx.api.creates.length, 1);
    const reads = fx.api.log.filter((e) => e.kind === 'status');
    assert.ok(reads.length >= 1);
    assert.ok(reads.every((e) => e.method === 'GET' && e.path.endsWith(`/${op}`)));
    // Start over: a new attempt with a new op id, saying it may make a second thread.
    await p.page.click('[data-part=launcher] [data-part=start-over]');
    l = await readLauncher(p.page);
    assert.match(l.note, /new op id: if the earlier create happened, this one makes a second thread/);
    assert.equal(l.readOnly, false);
    fx.api.setCreate((project, b) => json(200, nt.report(project, b, nt.OUTCOMES.created('thr_web_9002'))));
    await ready(p.page);
    await press(p.page);
    await settled(p.page, 'created');
    const ops = createPosts(fx).map((b) => b.op_id);
    assert.equal(ops[1], ops[0].replace(/-c1$/, '-c2'));
  } finally {
    await p.close();
    await fx.close();
  }
});

test('U12: double clicks and a held Ctrl+Enter send one create', async () => {
  const { fx } = await launcherSite();
  fx.api.hold('create');
  const p = await openLauncher(browser, fx);
  try {
    await fillAndReady(p.page);
    await p.page.dblclick('[data-part=launcher] [data-part=start]', { force: true });
    await press(p.page);
    await p.page.focus('[data-part=launcher] textarea');
    await p.page.keyboard.down('Control');
    for (let i = 0; i < 5; i++) await p.page.keyboard.down('Enter');
    await p.page.keyboard.up('Enter');
    await p.page.keyboard.up('Control');
    await waitFor(() => fx.api.held('create') > 0, { what: 'the create' });
    fx.api.hold('create', false);
    fx.api.release('create');
    await settled(p.page, 'created');
    assert.equal(creates(fx).length, 1, 'one request');
    assert.equal(fx.api.creates.length, 1, 'one provider call');
  } finally {
    await p.close();
    await fx.close();
  }
});

test('U12: a reload mid-create, then Try again and a copied tab: one op id, one provider call', async () => {
  const { fx } = await launcherSite();
  fx.api.setCreate(() => HANG);
  const p = await openLauncher(browser, fx);
  try {
    await fillAndReady(p.page);
    await press(p.page);
    await settled(p.page, 'moving');
    const op = createPosts(fx)[0].op_id;
    // The page reloads while the create is out; the server goes on and records it.
    fx.api.threadRecords.get(op).report = nt.report('prj_web_0001', createPosts(fx)[0], nt.OUTCOMES.created('thr_web_9001'));
    await p.page.reload();
    await waitFor(async () => (await p.page.locator('main [data-new-thread]').count()) > 0, { what: 'rows' });
    await pressNewThread(p.page);
    let l = await readLauncher(p.page);
    assert.equal(l.word, 'uncertain');
    // The reload ends the request: unknown, before or after the page goes (a create that was out, or not settled when it loaded).
    assert.match(l.lines.join(' '), /the page reloaded during the create|not settled when this page loaded/);
    // A copied tab: this tab's storage, copied, as a duplicated tab has it.
    const saved = await p.page.evaluate(() => JSON.stringify(Object.fromEntries(Object.entries(sessionStorage))));
    const copy = await p.context.newPage();
    await copy.addInitScript((s) => {
      for (const [k, v] of Object.entries(JSON.parse(s))) sessionStorage.setItem(k, v);
    }, saved);
    await copy.goto(fx.site.origin);
    await waitFor(async () => (await copy.locator('main [data-new-thread]').count()) > 0, { what: 'rows in the copy' });
    await pressNewThread(copy);
    for (const page of [p.page, copy]) {
      await page.click('[data-part=launcher] [data-part=start]', { force: true });
      l = await settled(page, 'created');
      assert.equal(l.thread, 'thr_web_9001');
    }
    assert.deepEqual([...new Set(createPosts(fx).map((b) => b.op_id))], [op], 'every request sent the one op id');
    assert.equal(fx.api.creates.length, 1, 'one provider call: the core replays');
    await copy.close();
  } finally {
    await p.close();
    await fx.close();
  }
});

test('U13: refreshes never touch the draft, the settings, the focus or a shown outcome, and never create', async () => {
  const { fx } = await launcherSite();
  const p = await openLauncher(browser, fx);
  try {
    await chooseAll(p.page);
    await typePrompt(p.page, PROMPT);
    await p.page.focus('[data-part=launcher] textarea');
    await p.page.evaluate(() => document.querySelector('[data-part=launcher] textarea').setSelectionRange(3, 7));
    const listReads = () => fx.api.log.filter((e) => e.kind === 'list').length;
    const n = listReads();
    await p.page.clock.runFor(TIMING.listIntervalMs * 3);
    await waitFor(() => listReads() >= n + 2, { what: 'two list refreshes' });
    let l = await readLauncher(p.page);
    assert.equal(l.draft, PROMPT);
    assert.deepEqual(l.picks.map((x) => x.value), ['codex', 'syn-codex-model-01', 'high']);
    assert.equal(l.focused, true, 'focus stays in the prompt');
    assert.deepEqual(await p.page.evaluate(() => {
      const t = document.querySelector('[data-part=launcher] textarea');
      return [t.selectionStart, t.selectionEnd];
    }), [3, 7], 'and the caret');
    assert.equal(fx.api.creates.length, 0);
    // A shown refusal stays through refreshes, and nothing is created.
    fx.api.setCreate(() => apiError(503, 'busy', 'too many requests in progress; retry later', { 'Retry-After': '5' }));
    await ready(p.page);
    await press(p.page);
    l = await settled(p.page, 'not-sent');
    const shown = [l.word, l.lines];
    await p.page.clock.runFor(TIMING.listIntervalMs * 3);
    await waitFor(() => listReads() >= n + 4, { what: 'more list refreshes' });
    l = await readLauncher(p.page);
    assert.deepEqual([l.word, l.lines], shown);
    assert.equal(creates(fx).length, 1, 'only the press created a request');
    // Created: the refresh reads the start (GET), bounded, and never creates.
    fx.api.setCreate((project, b) => json(200, nt.report(project, b, nt.OUTCOMES.created('thr_web_9001'), nt.start('thr_web_9001', 'waiting_to_start', b))));
    await press(p.page);
    await settled(p.page, 'created');
    fx.api.setStatus((op, known) => json(200, nt.status(op, known.report, nt.start('thr_web_9001', 'started', known))));
    await p.page.clock.runFor(TIMING.listIntervalMs * 2);
    l = await waitFor(async () => {
      const x = await readLauncher(p.page);
      return x.startLine?.startsWith('start: started') ? x : null;
    }, { what: 'the start, read again by the refresh' });
    // The busy refusal never reached the core: one provider call, the created one.
    assert.equal(fx.api.creates.length, 1);
    assert.equal(creates(fx).length, 2);
    assert.ok(fx.api.log.filter((e) => e.kind === 'status').every((e) => e.method === 'GET'));
  } finally {
    await p.close();
    await fx.close();
  }
});
