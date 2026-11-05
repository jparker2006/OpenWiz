

import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';

import { launchBrowser, waitFor } from '../support/browser.mjs';
import { apiError, bare, DROP, json } from '../support/fake-api.mjs';
import { chooseAll, createPosts, launcherSite, openLauncher, pressNewThread, PROMPT, readLauncher, ready, settled, typePrompt } from '../support/launcher.mjs';
import * as nt from '../support/newthread-synthetic.ts';

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(() => browser.close());

// `force`: Start thread is `aria-disabled` while a create is out.
const press = (page) => page.click('[data-part=launcher] [data-part=start]', { force: true });

/** Chooses codex / syn-codex-model-01 / high, writes the prompt, presses: the provider creates, the answer is lost. */
async function lostCreate(browser, fx) {
  fx.api.setCreate((project, b) => ({ after: DROP, report: nt.report(project, b, nt.OUTCOMES.created('thr_web_9001')) }));
  const p = await openLauncher(browser, fx);
  await chooseAll(p.page);
  await typePrompt(p.page, PROMPT);
  await ready(p.page);
  await press(p.page);
  const l = await settled(p.page, 'uncertain');
  assert.equal(l.start.label, 'Try again');
  assert.equal(fx.api.creates.length, 1, 'the provider created it; only the answer was lost');
  return p;
}

for (const [name, refusal, words] of [
  ['an OpenWiz 503 before the core', () => apiError(503, 'busy', 'too many requests in progress; retry later', { 'Retry-After': '5' }), /this Try again was refused \(busy: too many requests in progress; retry later\); the earlier create is still uncertain/],
  ['Access’s 401', () => apiError(401, 'signed_out', 'Access session expired'), /this Try again was refused \(signed out: sign in again, then Try again\); the earlier create is still uncertain/],
  ['Access’s redirect', () => bare(302, { Location: '/cdn-cgi/access/login' }), /this Try again was refused \(signed out: sign in again, then Try again\); the earlier create is still uncertain/],
]) {
  test(`R1: a lost create, then Try again refused by ${name}: still uncertain, the refusal on its own line`, async () => {
    const { fx } = await launcherSite();
    const p = await lostCreate(browser, fx);
    try {
      const shownBefore = await readLauncher(p.page);
      fx.api.setBeforeCore(refusal);
      await press(p.page);
      await waitFor(() => createPosts(fx).length === 2, { what: 'the Try again' });
      const l = await waitFor(async () => {
        const x = await readLauncher(p.page);
        return x.lines.some((line) => /this Try again was refused/.test(line)) ? x : null;
      }, { what: 'the refusal, shown' });
      assert.equal(l.outcome, 'uncertain', 'the shape stays uncertain');
      assert.equal(l.word, 'uncertain', 'the word stays uncertain');
      assert.deepEqual(l.lines.slice(0, shownBefore.lines.length), shownBefore.lines, 'the earlier create’s lines stay, in order');
      assert.equal(l.lines.filter((line) => words.test(line)).length, 1, 'the refusal, once, on its own line');
      const all = l.lines.join(' ');
      assert.doesNotMatch(all, /not created|nothing was created/, 'never "not created" while the first may have happened');
      assert.equal(l.start.label, 'Try again', 'Try again stays: the same op id, created at most once');
      assert.equal(l.readOnly, true);
      assert.deepEqual(l.unresolved, ['Refresh status', 'Start over']);
      const ops = createPosts(fx).map((b) => b.op_id);
      assert.equal(new Set(ops).size, 1, 'both requests carried the one op id');
      assert.equal(fx.api.creates.length, 1, 'the refused retry called nothing');
    } finally {
      await p.close();
      await fx.close();
    }
  });
}

/** After a lost create and a reload, opens the launcher again with `options` answering the options read. */
async function reloadWith(p, fx, options) {
  fx.api.setOptions(options);
  await p.page.reload();
  await waitFor(async () => (await p.page.locator('main [data-new-thread]').count()) > 0, { what: 'rows' });
  await pressNewThread(p.page);
  return readLauncher(p.page);
}

for (const [name, options, optionsWords, suffix] of [
  ['the options can’t be read', () => apiError(503, 'busy', 'too many requests in progress; retry later', { 'Retry-After': '5' }), /^options couldn’t be read: busy: too many requests in progress; retry later$/, 'options not read'],
  [
    'the catalogue no longer offers the saved choice',
    () => json(200, nt.options(undefined, { harnesses: nt.options().harnesses.map((h) => (h.harness === 'codex' ? { ...h, models: h.models.filter((m) => m.model !== 'syn-codex-model-01') } : h)) })),
    null,
    'not offered now',
  ],
]) {
  test(`R2: after a reload with an unresolved create, when ${name}, the requested settings stay visible and Try again sends them`, async () => {
    const { fx } = await launcherSite();
    const p = await lostCreate(browser, fx);
    try {
      const l = await reloadWith(p, fx, options);
      assert.equal(l.word, 'uncertain');
      assert.deepEqual(l.picks.map((x) => x.value), ['codex', 'syn-codex-model-01', 'high'], 'the controls hold the attempt’s settings');
      for (const x of l.picks) {
        assert.equal(x.shown.split(' ')[0], x.value, `${x.field}: the value is shown, not a placeholder`);
        assert.equal(x.disabled, true, `${x.field}: fixed while the create is unresolved`);
      }
      if (optionsWords) assert.match(await p.page.evaluate(() => document.querySelector('[data-part=options-failed]')?.textContent ?? ''), optionsWords, 'the options’ failure, on its own');
      const notOffered = l.picks.filter((x) => x.shown.includes(suffix));
      assert.ok(notOffered.length >= 1, `the settings the options don’t list say so (${suffix})`);
      assert.equal(l.start.label, 'Try again');
      assert.equal(l.start.disabled, false);
      await press(p.page);
      await settled(p.page, 'created');
      const bodies = createPosts(fx);
      assert.equal(bodies.length, 2);
      assert.deepEqual([bodies[1].harness, bodies[1].model, bodies[1].effort], l.picks.map((x) => x.value), 'Try again sent exactly the settings shown');
      assert.equal(bodies[1].op_id, bodies[0].op_id);
      assert.equal(fx.api.creates.length, 1, 'the core replayed: one provider call');
    } finally {
      await p.close();
      await fx.close();
    }
  });
}
