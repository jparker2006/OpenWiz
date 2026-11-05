// U7 to U10 : the launcher's settings, labels, effects and success, on
// the fake API's new-thread routes. Synthetic values only.
//
// - U7 visible, individual, honest settings: three controls in order, each
//   showing its choice in words; only offered options; an unavailable harness
//   listed with its reason and not choosable; a harness switch empties an
//   unsupported model or effort visibly, never substituting; nothing
//   preselected (the check); the create carries exactly the shown settings.
// - U8 requested is never verified: before creating, the settings say
//   requested; after, the reported configuration is labelled as such, beside
//   the requested one, and a difference is shown.
// - U9 effects come before pressing, in the core's words, from its preview;
//   Start thread waits for them; a preview never creates.
// - U10 success opens the new thread's details in the same panel or sheet,
//   with Created and the start stage on separate lines.

import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';

import { DESKTOP, launchBrowser, PHONE, readDetail, waitFor } from '../support/browser.mjs';
import { json } from '../support/fake-api.mjs';
import { chooseAll, createPosts, launcherSite, openLauncher, pick, PROJECT, PROMPT, readLauncher, ready, settled, typePrompt } from '../support/launcher.mjs';
import * as nt from '../support/newthread-synthetic.ts';

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(() => browser.close());

// `force`: Start thread is `aria-disabled` while it can't create, and a press then must do nothing.
const press = (page) => page.click('[data-part=launcher] [data-part=start]', { force: true });

for (const [name, viewport] of [
  ['phone', PHONE],
  ['desktop', DESKTOP],
]) {
  test(`U7 (${name}): nothing preselected; three controls in order, in words; only offered options`, async () => {
    const { fx } = await launcherSite();
    const p = await openLauncher(browser, fx, { viewport });
    try {
      const l = await readLauncher(p.page);
      assert.deepEqual(
        l.picks.map((x) => [x.field, x.label, x.value, x.shown]),
        [
          ['harness', 'harness', '', 'choose a harness'],
          ['model', 'model', '', 'choose a harness first'],
          ['effort', 'effort', '', 'choose a model first'],
        ],
        'nothing is preselected, and each empty control says so in words',
      );
      assert.deepEqual(l.picks[0].offered, ['codex', 'claude-code'], 'only the offered harnesses, in the core’s order');
      assert.deepEqual(l.picks[0].unavailable, ['cursor · unavailable (catalogue not reported)'], 'an unavailable harness is listed with its reason, not choosable');
      assert.deepEqual(l.picks[1].offered, []);
      assert.match(l.optionsLine, /^options read \d\d:\d\d:\d\d · as reported by CloudRoom; it may answer from a saved copy$/);
      await pick(p.page, 'harness', 'codex');
      let now = await readLauncher(p.page);
      assert.deepEqual(now.picks[1].offered, ['syn-codex-model-01', 'syn-codex-model-02', 'syn-shared-model']);
      assert.equal(now.picks[1].value, '', 'choosing a harness chooses no model');
      await pick(p.page, 'model', 'syn-codex-model-01');
      now = await readLauncher(p.page);
      assert.deepEqual(now.picks[2].offered, nt.EFFORTS);
      assert.equal(now.picks[2].value, '', 'nor an effort');
      // An unavailable harness can't be chosen: its option is disabled.
      const cursor = await p.page.evaluate(() => [...document.querySelectorAll('[data-field=harness] option')].find((o) => o.value === 'unavailable:cursor')?.disabled);
      assert.equal(cursor, true);
    } finally {
      await p.close();
      await fx.close();
    }
  });
}

test('U7: a switch keeps what the new harness offers and empties the rest visibly; the create carries exactly the shown settings', async () => {
  const { fx } = await launcherSite();
  const p = await openLauncher(browser, fx);
  try {
    await chooseAll(p.page, 'codex', 'syn-codex-model-01', 'high');
    await pick(p.page, 'harness', 'claude-code');
    let l = await readLauncher(p.page);
    assert.deepEqual(l.picks.map((x) => [x.value, x.shown]), [['claude-code', 'claude-code'], ['', 'choose a model'], ['', 'choose a model first']]);
    assert.match(l.note, /syn-codex-model-01 isn’t offered for claude-code: choose a model/);
    assert.equal(l.start.disabled, true);
    assert.match(l.startEffect, /can’t start: choose a model/);
    // A model both list stays; an effort only where offered with it.
    await pick(p.page, 'model', 'syn-shared-model');
    await pick(p.page, 'effort', 'max');
    await pick(p.page, 'harness', 'codex');
    l = await readLauncher(p.page);
    assert.deepEqual(l.picks.map((x) => x.value), ['codex', 'syn-shared-model', '']);
    assert.match(l.note, /max isn’t offered for syn-shared-model: choose an effort/);
    await typePrompt(p.page, 'Synthetic draft');
    assert.match((await readLauncher(p.page)).note, /max isn’t offered for syn-shared-model/, 'why a setting was emptied stays while the owner writes');
    await pick(p.page, 'effort', 'medium');
    await typePrompt(p.page, PROMPT);
    await ready(p.page);
    await press(p.page);
    await settled(p.page, 'created');
    const bodies = createPosts(fx);
    assert.equal(bodies.length, 1);
    assert.deepEqual(
      { harness: bodies[0].harness, model: bodies[0].model, effort: bodies[0].effort, text: bodies[0].text, attachments: bodies[0].attachments },
      { harness: 'codex', model: 'syn-shared-model', effort: 'medium', text: PROMPT, attachments: [] },
    );
    assert.deepEqual(fx.api.creates.map((c) => c.project), [PROJECT], 'the project of the heading pressed');
  } finally {
    await p.close();
    await fx.close();
  }
});

test('U9: the core’s effect lines come before pressing; Start thread waits for them; a preview creates nothing', async () => {
  const { fx } = await launcherSite();
  fx.api.hold('create-preview');
  const p = await openLauncher(browser, fx);
  try {
    let l = await readLauncher(p.page);
    assert.deepEqual(l.effects, []);
    assert.match(l.startEffect, /can’t start: choose a harness/);
    await chooseAll(p.page);
    await typePrompt(p.page, PROMPT);
    await waitFor(() => fx.api.held('create-preview') > 0, { what: 'the preview read' });
    l = await readLauncher(p.page);
    assert.equal(l.start.disabled, true);
    assert.match(l.startEffect, /can’t start: reading what Start thread would do/);
    // Pressing now does nothing.
    await press(p.page);
    fx.api.hold('create-preview', false);
    fx.api.release('create-preview');
    l = await ready(p.page);
    assert.deepEqual(l.effects, nt.effects(PROJECT).effects.map((e) => e.text), 'the core’s words, in its order');
    assert.match(l.effectsAge, /^effects read \d+s ago$/);
    assert.equal(fx.api.creates.length, 0, 'no preview, and no early press, created anything');
    const previews = fx.api.log.filter((e) => e.kind === 'create-preview');
    assert.ok(previews.length >= 1);
    assert.equal(JSON.parse(previews.at(-1).body).text, PROMPT);
  } finally {
    await p.close();
    await fx.close();
  }
});

test('U9: a create the core would stop is shown before pressing, with its reason, and can’t be pressed', async () => {
  const { fx } = await launcherSite();
  fx.api.setCreatePreview((project, b) => json(200, nt.preview(project, b, nt.OUTCOMES.noRemote())));
  const p = await openLauncher(browser, fx);
  try {
    await chooseAll(p.page);
    await typePrompt(p.page, PROMPT);
    const l = await waitFor(async () => {
      const x = await readLauncher(p.page);
      return x.preview === 'ok' ? x : null;
    }, { what: 'the preview' });
    assert.equal(l.start.disabled, true);
    assert.match(l.startEffect, /can’t start: refused: the project has no GitHub remote/);
    await press(p.page);
    assert.equal(fx.api.creates.length, 0);
  } finally {
    await p.close();
    await fx.close();
  }
});

for (const [name, viewport, modal] of [
  ['phone: the sheet', PHONE, true],
  ['desktop: the panel', DESKTOP, false],
]) {
  test(`U8, U10 (${name}): created opens the new thread’s details in the same place; created and start apart; requested beside reported`, async () => {
    const { fx } = await launcherSite();
    fx.api.setCreate((project, b) => json(200, nt.report(project, b, nt.OUTCOMES.created('thr_web_9001'), nt.start('thr_web_9001', 'waiting_to_start', b, undefined, 'syn-codex-model-02'))));
    const p = await openLauncher(browser, fx, { viewport });
    try {
      let l = await readLauncher(p.page);
      assert.ok(l.buttons.length > 0);
      const before = await p.page.evaluate(() => document.querySelector('[data-part=launcher] .sec')?.textContent);
      assert.equal(before, 'requested settings', 'before creating, the settings say requested');
      await chooseAll(p.page);
      await typePrompt(p.page, PROMPT);
      await ready(p.page);
      await press(p.page);
      l = await settled(p.page, 'created');
      const d = await readDetail(p.page);
      assert.equal(d.modal, modal, 'the same panel or sheet');
      assert.equal(await p.page.evaluate(() => document.querySelectorAll('dialog').length), 1);
      assert.equal(l.thread, 'thr_web_9001');
      assert.equal(l.word, 'created');
      assert.equal(l.created, nt.CREATED_WORDS, 'created, in the core’s words');
      assert.match(l.startLine, /^start: waiting to start · read \d\d:\d\d:\d\d$/, 'the start stage, on its own line');
      assert.equal(l.requested, 'harness codex · model syn-codex-model-01 · effort high');
      assert.equal(l.reportedLabel, 'reported configuration: what CloudRoom reports, never the serving model');
      assert.deepEqual(l.reported, ['provider codex, as requested', 'model syn-codex-model-02: differs from requested (syn-codex-model-01)', 'effort high, as requested']);
      // Never claimed as the serving model.
      const all = await p.page.evaluate(() => document.querySelector('[data-part=created]').textContent);
      assert.doesNotMatch(all.replace('never the serving model', ''), /serving|running model|verified/i);
    } finally {
      await p.close();
      await fx.close();
    }
  });
}
