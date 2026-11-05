// AW9: reloads and reconnects. Reloading mid-upload, losing the network
// and signing out each keep the draft and dispatch nothing; nothing retries by
// itself; a ready attachment comes back after a reload, without a preview.
// AW10: drag and drop onto the prompt body adds files, a folder is refused
// with its word, and a drop elsewhere never opens the file. AW11: keyboard,
// screen-reader names for each state, 44-point targets, and states told apart
// in greyscale.

import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';

import { launchBrowser, PHONE, settleReal, waitFor } from '../support/browser.mjs';
import { composerSite, openThread, posts, readComposer, select, type } from '../support/composer.mjs';
import { DROP, HANG, json } from '../support/fake-api.mjs';
import { axe } from '../support/harness.mjs';
import { addFiles, COMPOSER, png, readTray, text, trayIs } from '../support/tray.mjs';

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(() => browser.close());

test('AW9: a reload mid-upload keeps the draft, dispatches nothing, and says the file is gone; a ready one comes back without a preview', async () => {
  const { fx } = await composerSite();
  let first = true;
  // The first upload is ready; the second never answers (the page reloads while it uploads).
  fx.api.setUpload((name, bytes) => {
    if (first) {
      first = false;
      const r = { id: `att-${'a'.repeat(32)}`, sha256: '0'.repeat(64), size_bytes: bytes.length, kind: 'image', name, display_name: name };
      fx.api.staged.set(r.id, r);
      return json(200, { schema: 'openwiz.attachment-add.v1', ready: [{ index: 0, path: name, staged: { attachment: r, readiness: { state: 'ready' }, ready_at: 1, age_ms: 0, expires_at: 2 } }], failed: [], not_staged: [], expired: [] });
    }
    return HANG;
  });
  const p = await openThread(browser, fx, 1);
  try {
    await type(p.page, 'Synthetic draft kept');
    await addFiles(p.page, COMPOSER, [{ name: 'a.png', buffer: png(), mimeType: 'image/png' }]);
    await trayIs(p.page, COMPOSER, 1, 'ready');
    await addFiles(p.page, COMPOSER, [{ name: 'b.txt', buffer: text() }]);
    await waitFor(async () => (await readTray(p.page, COMPOSER)).rows[1]?.state === 'uploading', { what: 'uploading' });
    const uploads = fx.api.uploads.length;
    await p.page.reload();
    await waitFor(async () => (await p.page.locator('main [data-session-id]').count()) > 0, { what: 'rows after the reload' });
    await select(p.page, 1);
    const t = await waitFor(async () => {
      const t = await readTray(p.page, COMPOSER);
      return t?.rows.length === 2 && t.rows[1].state === 'gone' ? t : null;
    }, { what: 'the tray after the reload' });
    assert.equal((await readComposer(p.page)).draft, 'Synthetic draft kept', 'the draft is kept');
    assert.deepEqual(
      t.rows.map((r) => [r.name, r.state, r.thumb]),
      [
        ['a.png', 'ready', 'no'],
        ['b.txt', 'gone', 'no'],
      ],
    );
    assert.equal(t.rows[0].reload, 'preview not available after reload');
    assert.match(t.rows[1].why, /not uploaded before this page reloaded/);
    assert.equal(t.rows[1].retry, false, 'no Retry: the browser no longer has the file');
    assert.equal((await readComposer(p.page)).off, 'can’t send: 1 attachment not ready');
    await settleReal(500);
    assert.equal(fx.api.uploads.length, uploads, 'a reload never uploads again');
    assert.equal(posts(fx).length, 0, 'and never sends');
    assert.deepEqual(fx.api.log.filter((e) => e.kind === 'attachments' && e.method === 'GET').length, 1, 'one read of the list, to check the ready one');
  } finally {
    await p.close();
    await fx.close();
  }
});

test('AW9: a lost connection and a signed-out session each fail the upload in words, keep the draft, and never retry by themselves', async () => {
  const { fx } = await composerSite();
  let n = 0;
  fx.api.setUpload(() => (n++ === 0 ? DROP : { status: 401, headers: { 'Content-Type': 'text/html' }, body: '<html>Access</html>' }));
  const p = await openThread(browser, fx, 1);
  try {
    await type(p.page, 'Synthetic draft kept');
    await addFiles(p.page, COMPOSER, [{ name: 'a.txt', buffer: text() }]);
    let t = await waitFor(async () => {
      const t = await readTray(p.page, COMPOSER);
      return t?.rows[0]?.state === 'failed' ? t : null;
    }, { what: 'connection lost' });
    assert.match(t.rows[0].why, /^connection lost: /);
    assert.equal(t.rows[0].retry, true);
    const before = fx.api.log.filter((e) => e.kind === 'attachments' && e.method === 'POST').length;
    assert.equal(before, 1, 'one upload');
    await settleReal(500);
    assert.equal(fx.api.log.filter((e) => e.kind === 'attachments' && e.method === 'POST').length, before, 'no upload by itself');
    // Retry is the owner's: it uploads again, and this time Access says signed out.
    await p.page.locator(`${COMPOSER} [data-part=attachment-retry]`).click();
    t = await waitFor(async () => {
      const t = await readTray(p.page, COMPOSER);
      return t?.rows[0]?.state === 'failed' && /^signed out: /.test(t.rows[0].why) ? t : null;
    }, { what: 'signed out' });
    assert.match(t.rows[0].why, /^signed out: /);
    assert.equal((await readComposer(p.page)).draft, 'Synthetic draft kept');
    assert.equal(posts(fx).length, 0, 'nothing is sent');
  } finally {
    await p.close();
    await fx.close();
  }
});

/** Drops `files` (and folders, by name) on the element `selector`, as a desktop drag does; whether each event's default was stopped. */
function drop(page, selector, files, folders = []) {
  return page.evaluate(
    ({ selector, files, folders }) => {
      const el = document.querySelector(selector);
      const dt = new DataTransfer();
      for (const f of [...files, ...folders]) dt.items.add(new File([f.text], f.name, { type: f.type ?? '' }));
      // A synthetic DataTransfer has no file system entries: mark the folders as a real drop would.
      const real = DataTransferItem.prototype.webkitGetAsEntry;
      DataTransferItem.prototype.webkitGetAsEntry = function () {
        const file = this.getAsFile();
        return folders.some((f) => f.name === file?.name) ? { isDirectory: true } : { isDirectory: false };
      };
      const over = new DragEvent('dragover', { dataTransfer: dt, bubbles: true, cancelable: true });
      el.dispatchEvent(over);
      const d = new DragEvent('drop', { dataTransfer: dt, bubbles: true, cancelable: true });
      el.dispatchEvent(d);
      DataTransferItem.prototype.webkitGetAsEntry = real;
      return [over.defaultPrevented, d.defaultPrevented];
    },
    { selector, files, folders },
  );
}

test('AW10: a drop on the prompt body attaches its files; a folder is refused with its word; a drop elsewhere never opens the file', async () => {
  const { fx } = await composerSite();
  const p = await openThread(browser, fx, 1);
  try {
    const url = p.page.url();
    assert.deepEqual(await drop(p.page, `${COMPOSER} textarea`, [{ name: 'a.txt', text: 'Synthetic a' }]), [true, true]);
    await trayIs(p.page, COMPOSER, 1, 'ready');
    assert.deepEqual(await drop(p.page, `${COMPOSER} [data-part=tray]`, [{ name: 'b.txt', text: 'Synthetic b' }], [{ name: 'synthetic-folder', text: '' }]), [true, true]);
    const t = await trayIs(p.page, COMPOSER, 2, 'ready');
    assert.deepEqual(t.rows.map((r) => r.name), ['a.txt', 'b.txt'], 'the folder is not attached');
    assert.equal(t.note, 'Folders aren’t supported yet.');
    assert.deepEqual(fx.api.uploads.map((u) => u.name), ['a.txt', 'b.txt']);
    // Outside the prompt body: the list, the page's header, the detail's own heading.
    for (const where of ['main', 'header', '[data-part=detail] h2']) {
      const [over, dropped] = await drop(p.page, where, [{ name: 'c.txt', text: 'Synthetic c' }]);
      assert.equal(dropped, true, `${where}: the drop's default (opening the file) is stopped`);
      assert.equal(over, true, `${where}: the drag is refused`);
    }
    await settleReal(300);
    assert.equal(p.page.url(), url, 'the page never navigated');
    assert.deepEqual(fx.api.uploads.map((u) => u.name), ['a.txt', 'b.txt'], 'nothing dropped outside is attached');
    // While an attempt may have been sent, the attachments are fixed: a drop is refused, in words.
    fx.api.setSend(() => DROP);
    await type(p.page, 'Synthetic message');
    await p.page.locator(`${COMPOSER} [data-part=send]`).first().click();
    await waitFor(async () => (await readComposer(p.page)).readOnly, { what: 'locked' });
    await drop(p.page, `${COMPOSER} textarea`, [{ name: 'd.txt', text: 'Synthetic d' }]);
    assert.equal((await readComposer(p.page)).note, 'attachments are fixed while this attempt may have been sent. Nothing was attached');
    assert.equal((await readTray(p.page, COMPOSER)).rows.length, 2);
  } finally {
    await p.close();
    await fx.close();
  }
});

test('AW11: keyboard, names for each state, 44-point targets, no axe violation, and states told apart without colour', async () => {
  const { fx } = await composerSite();
  let n = 0;
  fx.api.setUpload((name, bytes) => {
    n++;
    if (n === 2) return HANG;
    if (n === 3) return json(200, { schema: 'openwiz.attachment-add.v1', ready: [], failed: [{ index: 0, path: name, readiness: { state: 'failed', reason: 'file_too_large' }, message: 'it is over 25 MiB, CloudRoom’s limit per file' }], not_staged: [], expired: [] });
    const r = { id: `att-${String(n).padStart(32, '0')}`, sha256: '0'.repeat(64), size_bytes: bytes.length, kind: 'file', name, display_name: name };
    fx.api.staged.set(r.id, r);
    return json(200, { schema: 'openwiz.attachment-add.v1', ready: [{ index: 0, path: name, staged: { attachment: r, readiness: { state: 'ready' }, ready_at: 1, age_ms: 0, expires_at: 2 } }], failed: [], not_staged: [], expired: [] });
  });
  for (const viewport of [PHONE, undefined]) {
    n = 0;
    const p = await openThread(browser, fx, 1, viewport ? { viewport } : {});
    try {
      await type(p.page, 'Synthetic message');
      await addFiles(p.page, COMPOSER, [
        { name: 'ready.txt', buffer: text() },
        { name: 'uploading.txt', buffer: text() },
      ]);
      await addFiles(p.page, COMPOSER, [{ name: 'failed.txt', buffer: text() }]);
      const t = await waitFor(async () => {
        const t = await readTray(p.page, COMPOSER);
        return t?.rows.length === 3 && t.rows.map((r) => r.state).join() === 'ready,uploading,failed' ? t : null;
      }, { what: 'three states' });
      assert.deepEqual(
        t.rows.map((r) => [r.word, r.glyph]),
        [
          ['ready on your controller', '[✓]'],
          ['uploading to your controller', '[~]'],
          ['failed', '[x]'],
        ],
      );
      assert.deepEqual(t.rows.map((r) => r.remove), ['Remove ready.txt', 'Remove uploading.txt', 'Remove failed.txt']);
      // Every control is at least 44 by 44.
      const small = await p.page.evaluate((scope) =>
        [...document.querySelectorAll(`${scope} [data-part=tray] button`)].map((b) => b.getBoundingClientRect()).filter((r) => r.width < 44 || r.height < 44).length, COMPOSER);
      assert.equal(small, 0, 'every tray control is at least 44 px');
      // Greyscale: each state's marker differs in its shape, not only its colour.
      const shapes = await p.page.evaluate((scope) =>
        [...document.querySelectorAll(`${scope} [data-part=attachment-state]`)].map((m) => {
          const s = getComputedStyle(m);
          const b = getComputedStyle(m, '::before');
          return [s.fontWeight, s.fontStyle, s.backgroundColor !== 'rgba(0, 0, 0, 0)', b.boxShadow, b.borderStyle].join('|');
        }), COMPOSER);
      assert.equal(new Set(shapes).size, 3, `three different shapes: ${shapes.join(' / ')}`);
      // Keyboard: Add files, then each row's Retry and Remove, in order.
      await p.page.locator(`${COMPOSER} textarea`).focus();
      const order = [];
      for (let i = 0; i < 12 && order.length < 5; i++) {
        await p.page.keyboard.press('Tab');
        const at = await p.page.evaluate(() => (document.activeElement?.closest('[data-part=tray]') ? (document.activeElement.getAttribute('aria-label') ?? document.activeElement.textContent) : null));
        if (at && !order.includes(at)) order.push(at);
      }
      assert.deepEqual(order, ['Add files', 'Remove ready.txt', 'Remove uploading.txt', 'Retry', 'Remove failed.txt']);
      // Enter on a focused Remove removes it.
      await p.page.locator(`${COMPOSER} [aria-label="Remove ready.txt"]`).focus();
      await p.page.keyboard.press('Enter');
      await waitFor(async () => (await readTray(p.page, COMPOSER)).rows.length === 2, { what: 'removed from the keyboard' });
      // Its button went with it: focus goes to the message field, never to the page.
      const focused = await waitFor(() => p.page.evaluate(() => (document.activeElement === document.body ? null : document.activeElement?.tagName)), { what: 'focus placed' });
      assert.equal(focused, 'TEXTAREA', 'focus goes to the message field');
      assert.deepEqual(await axe(p.page), [], 'no axe violation');
    } finally {
      await p.close();
    }
  }
  await fx.close();
});
