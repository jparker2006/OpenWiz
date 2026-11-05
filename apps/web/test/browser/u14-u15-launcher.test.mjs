

import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';

import { DESKTOP, launchBrowser, PHONE, readDetail, waitFor } from '../support/browser.mjs';
import { json } from '../support/fake-api.mjs';
import { axe } from '../support/harness.mjs';
import { chooseAll, launcherSite, openLauncher, PROJECT, PROMPT, readLauncher, ready, settled, typePrompt } from '../support/launcher.mjs';
import * as nt from '../support/newthread-synthetic.ts';

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(() => browser.close());

const placement = (page) =>
  page.evaluate(() => ({
    dialogs: document.querySelectorAll('dialog').length,
    launchers: document.querySelectorAll('[data-part=launcher]').length,
    inDetail: !!document.querySelector('[data-part=launcher]')?.closest('dialog[data-part=detail]'),
    textareas: document.querySelectorAll('textarea').length,
    composers: document.querySelectorAll('[data-part=composer]').length,
  }));

for (const [name, viewport, modal] of [
  ['phone: in the bottom sheet', PHONE, true],
  ['desktop: in the side panel', DESKTOP, false],
]) {
  test(`U14 (${name}): one dialog, one prompt body; the composer is unchanged under a session`, async () => {
    const { fx } = await launcherSite();
    const p = await openLauncher(browser, fx, { viewport });
    try {
      assert.equal((await readDetail(p.page)).modal, modal);
      assert.deepEqual(await placement(p.page), { dialogs: 1, launchers: 1, inDetail: true, textareas: 1, composers: 0 });
      // The prompt body is the composer's: the same field class and part.
      const field = await p.page.evaluate(() => {
        const t = document.querySelector('[data-part=launcher] textarea');
        return { cls: t.className, part: t.dataset.part, label: document.querySelector(`label[for="${t.id}"]`)?.textContent };
      });
      assert.deepEqual(field, { cls: 'compose', part: 'draft', label: 'prompt' });
      // A session's details take the same dialog back, with its composer under the final output.
      if (modal) await p.page.click('[data-part=detail] .sheet-x');
      await p.page.click('main [data-session-id="thr_web_0001"]');
      await waitFor(async () => (await readDetail(p.page))?.id === 'thr_web_0001', { what: 'the session' });
      await waitFor(() => p.page.evaluate(() => document.querySelector('[data-part=composer]') !== null), { what: 'its composer' });
      const after = await p.page.evaluate(() => ({
        dialogs: document.querySelectorAll('dialog').length,
        launchers: document.querySelectorAll('[data-part=launcher]').length,
        composers: document.querySelectorAll('[data-part=composer]').length,
        before: document.querySelector('[data-part=composer]')?.previousElementSibling?.getAttribute('data-field') ?? null,
      }));
      assert.deepEqual(after, { dialogs: 1, launchers: 0, composers: 1, before: 'final-output' });
    } finally {
      await p.close();
      await fx.close();
    }
  });
}


test('U15: a dropped file is attached, never text; a pasted path is text', async () => {
  const { fx } = await launcherSite();
  const p = await openLauncher(browser, fx);
  try {
    await p.page.evaluate(() => {
      const t = document.querySelector('[data-part=launcher] textarea');
      const dt = new DataTransfer();
      dt.items.add(new File(['synthetic'], 'synthetic.txt', { type: 'text/plain' }));
      t.dispatchEvent(new DragEvent('drop', { dataTransfer: dt, bubbles: true, cancelable: true }));
    });
    const l = await readLauncher(p.page);
    assert.equal(l.note, null);
    assert.equal(l.draft, '');
    await waitFor(() => fx.api.uploads.length === 1, { what: 'the dropped file uploaded' });
    assert.equal(await p.page.locator('[data-part=launcher] [data-part=attachment]').count(), 1);
    await typePrompt(p.page, '/Users/synthetic/notes.txt');
    assert.equal((await readLauncher(p.page)).draft, '/Users/synthetic/notes.txt');
  } finally {
    await p.close();
    await fx.close();
  }
});

test('U15: keyboard only: New thread, the three controls, the prompt and Ctrl+Enter; focus returns to New thread on close', async () => {
  const { fx } = await launcherSite();
  const p = await openLauncher(browser, fx);
  try {
    await p.page.click('[data-part=detail] .sheet-x');
    // Closed, as the page has it: its heading's New thread no longer marked open.
    await waitFor(() => p.page.evaluate(() => document.querySelector('main [data-new-thread]')?.getAttribute('aria-expanded') === 'false'), { what: 'closed' });
    const focused = await p.page.evaluate(() => document.activeElement?.dataset.newThread ?? null);
    assert.equal(focused, PROJECT, 'focus returns to the heading’s New thread');
    await p.page.keyboard.press('Enter');
    await waitFor(async () => (await readLauncher(p.page))?.options === 'read', { what: 'the launcher' });
    // A closed native select takes type-ahead: the first letters of an option choose it.
    for (const [field, value, keys] of [
      ['harness', 'codex', 'co'],
      ['model', 'syn-codex-model-01', 's'],
      ['effort', 'high', 'h'],
    ]) {
      await p.page.focus(`[data-part=pick][data-field=${field}] select`);
      await p.page.keyboard.type(keys);
      assert.equal((await readLauncher(p.page)).picks.find((x) => x.field === field).value, value, `${field} by keyboard`);
    }
    await p.page.focus('[data-part=launcher] textarea');
    await p.page.keyboard.type(PROMPT);
    await ready(p.page);
    await p.page.keyboard.press('Control+Enter');
    await settled(p.page, 'created');
    assert.equal(fx.api.creates.length, 1);
  } finally {
    await p.close();
    await fx.close();
  }
});

test('U15: provider strings render as plain text; outcomes are a word and a shape; no axe violations', async () => {
  const hostile = '<img src=x onerror=alert(1)>‮syn';
  const { fx } = await launcherSite();
  fx.api.setOptions(() =>
    json(200, nt.options(undefined, { unavailable: [{ harness: 'syn-hostile', reason: 'catalogue_not_reported', text: `unavailable (${hostile})` }] })),
  );
  fx.api.setCreate((project, b) => json(200, { ...nt.report(project, b, nt.OUTCOMES.uncertain()), summary: `${hostile} · uncertain` }));
  const p = await openLauncher(browser, fx);
  try {
    let l = await readLauncher(p.page);
    assert.deepEqual(l.picks[0].unavailable, [`syn-hostile · unavailable (${hostile})`], 'as plain text');
    assert.deepEqual(await axe(p.page), [], 'the empty launcher');
    await chooseAll(p.page);
    await typePrompt(p.page, PROMPT);
    await ready(p.page);
    assert.deepEqual(await axe(p.page), [], 'the ready launcher');
    await p.page.click('[data-part=launcher] [data-part=start]');
    l = await settled(p.page, 'uncertain');
    assert.equal(l.lines[0], `${hostile} · uncertain`, 'as plain text');
    assert.equal(l.live, 'status', 'a polite live region');
    const html = await p.page.evaluate(() => ({ imgs: document.querySelectorAll('[data-part=launcher] img').length, csp: window.__csp }));
    assert.deepEqual(html, { imgs: 0, csp: [] });
    const marker = await p.page.evaluate(() => {
      const m = document.querySelector('[data-part=launch-outcome] [data-part=outcome-marker]');
      const before = getComputedStyle(m, '::before');
      return { word: m.querySelector('.word').textContent, glyph: m.querySelector('.glyph').textContent, hidden: m.querySelector('.glyph').getAttribute('aria-hidden'), border: before.borderTopStyle, italic: getComputedStyle(m).fontStyle };
    });
    assert.deepEqual(marker, { word: 'uncertain', glyph: '[?]', hidden: 'true', border: 'dashed', italic: 'italic' }, 'a word, a glyph and a dashed, italic shape');
    assert.deepEqual(await axe(p.page), [], 'the uncertain launcher');
  } finally {
    await p.close();
    await fx.close();
  }
});
