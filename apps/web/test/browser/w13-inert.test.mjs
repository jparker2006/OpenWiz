

import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';

import { launchBrowser, PHONE, waitFor } from '../support/browser.mjs';
import { axe } from '../support/harness.mjs';
import { apiError, DROP, json } from '../support/fake-api.mjs';
import { button, composerSite, id, openThread, outcome, posts, readComposer, TEXT, type } from '../support/composer.mjs';
import { DELIVERIES, HOSTILE, outcome as result, report } from '../support/synthetic.ts';

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(() => browser.close());

test('one file input, the tray’s; a dropped file is attached, never text; a pasted file is refused with words; a pasted path is text', async () => {
  const { fx } = await composerSite();
  const p = await openThread(browser, fx, 1);
  try {
    assert.equal(await p.page.locator('[contenteditable]').count(), 0, 'no rich editor');
    assert.equal(await p.page.locator('input[type=file]').count(), 1, 'one file input: the tray’s');
    assert.equal(await p.page.locator('[data-part=composer] [data-part=tray] input[type=file]').count(), 1);
    await type(p.page, TEXT);
    const prevented = await p.page.evaluate(() => {
      const t = document.querySelector('[data-part=composer] textarea');
      const dt = new DataTransfer();
      dt.items.add(new File(['synthetic'], 'synthetic.txt', { type: 'text/plain' }));
      const over = new DragEvent('dragover', { dataTransfer: dt, bubbles: true, cancelable: true });
      t.dispatchEvent(over);
      const drop = new DragEvent('drop', { dataTransfer: dt, bubbles: true, cancelable: true });
      t.dispatchEvent(drop);
      return [over.defaultPrevented, drop.defaultPrevented];
    });
    assert.deepEqual(prevented, [true, true], 'the drop is taken by the prompt body');
    let c = await readComposer(p.page);
    assert.equal(c.note, null);
    assert.equal(c.draft, TEXT, 'the draft is unchanged: a file is never text');
    await waitFor(() => fx.api.uploads.length === 1, { what: 'the dropped file uploaded' });
    assert.equal(fx.api.uploads[0].name, 'synthetic.txt');
    await p.page.locator('[data-part=composer] [data-part=attachment-remove]').click();
    await waitFor(async () => (await p.page.locator('[data-part=composer] [data-part=attachment]').count()) === 0, { what: 'removed' });
    const pastedFile = await p.page.evaluate(() => {
      const t = document.querySelector('[data-part=composer] textarea');
      const dt = new DataTransfer();
      dt.items.add(new File(['synthetic'], 'synthetic.png', { type: 'image/png' }));
      const e = new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true });
      t.dispatchEvent(e);
      return e.defaultPrevented;
    });
    assert.equal(pastedFile, true, 'a pasted file alone is refused');
    assert.equal((await readComposer(p.page)).note, 'a file can’t be pasted: use Add files, or drop it here. Nothing was attached');
    // A file path is text: it pastes as text.
    await type(p.page, '');
    await p.page.locator('[data-part=composer] textarea').focus();
    await p.page.keyboard.insertText('/Users/synthetic/notes/plan.md');
    c = await readComposer(p.page);
    assert.equal(c.draft, '/Users/synthetic/notes/plan.md');
    await button(p.page, 'Send').click();
    await outcome(p.page, 'sent');
    assert.equal(posts(fx)[0].text, '/Users/synthetic/notes/plan.md', 'sent as text');
    assert.deepEqual(Object.keys(posts(fx)[0]).sort(), ['accept_possible_duplicate', 'mode', 'op_id', 'resume_paused_queue', 'text']);
  } finally {
    await p.close();
    await fx.close();
  }
});

test('keyboard: Tab reaches the message and the buttons, Ctrl+Enter sends, names say what each does', async () => {
  const { fx } = await composerSite({ kinds: { [id(1)]: 'busy' } });
  const p = await openThread(browser, fx, 1);
  try {
    const names = await p.page.evaluate(() => {
      const c = document.querySelector('[data-part=composer]');
      const t = c.querySelector('textarea');
      return {
        label: t.labels?.[0]?.textContent ?? null,
        status: c.querySelector('[data-part=outcome]')?.getAttribute('role'),
        buttons: [...c.querySelectorAll('button')].map((b) => b.textContent),
      };
    });

    assert.deepEqual(names, { label: 'message', status: 'status', buttons: ['Add files', 'Queue', 'Steer'] });
    await p.page.locator('[data-part=composer] textarea').focus();
    await p.page.keyboard.type(TEXT);
    await p.page.locator('[data-part=detail] .sheet-x').focus();
    const order = [];
    for (let i = 0; i < 40 && order.length < 4; i++) {
      await p.page.keyboard.press('Tab');
      const at = await p.page.evaluate(() => (document.activeElement?.closest('[data-part=composer]') ? (document.activeElement.tagName === 'BUTTON' ? document.activeElement.textContent : document.activeElement.tagName) : null));
      if (at && !order.includes(at)) order.push(at);
    }
    assert.deepEqual(order, ['TEXTAREA', 'Add files', 'Queue', 'Steer'], 'Tab order: the message, the tray’s Add files, then each button');
    await p.page.locator('[data-part=composer] textarea').focus();
    await p.page.keyboard.press('End');
    await p.page.keyboard.press('Enter');
    assert.equal((await readComposer(p.page)).draft, `${TEXT}\n`, 'Enter alone is a new line');
    await p.page.keyboard.press('Control+Enter');
    await outcome(p.page, 'sent');
    assert.equal(posts(fx)[0].mode, 'queue', 'Ctrl+Enter presses the first button: the default, Queue');
  } finally {
    await p.close();
    await fx.close();
  }
});

test('axe finds no violation in the composer’s states, phone and desktop', async () => {
  for (const viewport of [PHONE, null]) {
    const { fx } = await composerSite({ kinds: { [id(1)]: 'busy', [id(2)]: 'paused' } });
    let n = 0;
    fx.api.setSend((tid, b) => [
      () => json(200, report(tid, b.op_id, b.mode, b.text, result(DELIVERIES.unsupported))),
      () => ({ after: DROP, report: report(tid, b.op_id, b.mode, b.text, result(DELIVERIES.sent)) }),
    ][n++ % 2]());
    const p = await openThread(browser, fx, 1, viewport ? { viewport } : {});
    try {
      assert.deepEqual(await axe(p.page), [], 'idle composer');
      await type(p.page, TEXT);
      await button(p.page, 'Steer').click();
      await outcome(p.page, 'unsupported');
      assert.deepEqual(await axe(p.page), [], 'unsupported');
      await button(p.page, 'Queue').click();
      await outcome(p.page, 'uncertain');
      assert.deepEqual(await axe(p.page), [], 'uncertain');
    } finally {
      await p.close();
      await fx.close();
    }
  }
});

test('the core’s and the provider’s strings render as plain text and run nothing', async () => {
  const { fx } = await composerSite();
  const evil = Object.values(HOSTILE).join(' ');
  fx.api.setSend((tid, b) => {
    const r = report(tid, b.op_id, b.mode, b.text, result({ ...DELIVERIES.uncertain, text: evil, attribution: HOSTILE.html }));
    return json(200, { ...r, summary: `${HOSTILE.script} ${HOSTILE.link}` });
  });
  const p = await openThread(browser, fx, 1);
  try {
    await type(p.page, HOSTILE.html);
    await button(p.page, 'Send').click();
    const c = await outcome(p.page, 'uncertain');
    assert.equal(c.lines[0], `${HOSTILE.script} ${HOSTILE.link}`, 'shown as text, word for word');
    assert.ok(c.lines.includes(HOSTILE.html));
    const dom = await p.page.evaluate(() => ({
      pwned: window.__pwned ?? null,
      elements: document.querySelectorAll('[data-part=composer] img, [data-part=composer] script, [data-part=composer] a, [data-part=composer] b').length,
    }));
    assert.deepEqual(dom, { pwned: null, elements: 0 });
    assert.deepEqual(await p.csp(), []);
    // An error's words, too.
    fx.api.setSend(() => apiError(400, 'invalid_request', HOSTILE.html));
    await button(p.page, 'Forget it').click();
    await type(p.page, `${TEXT} two`);
    await button(p.page, 'Send').click();
    const e = await outcome(p.page, 'not-sent');
    assert.equal(e.lines[0], `invalid_request: ${HOSTILE.html}`);
    assert.equal(await p.page.evaluate(() => window.__pwned ?? null), null);
    await waitFor(async () => (await readComposer(p.page)).draft === `${TEXT} two`, { what: 'the text kept' });
  } finally {
    await p.close();
    await fx.close();
  }
});
