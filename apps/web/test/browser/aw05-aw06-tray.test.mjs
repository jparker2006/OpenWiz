// AW5, the page's half: the action button is disabled while any
// attachment is selected, uploading or failed, and says why ("2 attachments
// not ready"); Steer is not offered with attachments. The server's gate is the
// real one (tests/web-api, AW5). AW6: honest states. Selected never says
// uploaded; after an outcome the tray says how many were handed to CloudRoom,
// never "delivered" or "read". In the composer and in the launcher.

import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';

import { launchBrowser, waitFor } from '../support/browser.mjs';
import { button, composerSite, id, openThread, outcome, posts, readComposer, type } from '../support/composer.mjs';
import { chooseAll, createPosts, launcherSite, openLauncher, readLauncher, settled, typePrompt } from '../support/launcher.mjs';
import { addFiles, COMPOSER, LAUNCHER, png, readTray, text, trayIs } from '../support/tray.mjs';

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(() => browser.close());

const sendButton = async (page) => (await readComposer(page)).actions.find((a) => a.key === 'send') ?? null;

test('AW5, AW6: in the composer, Send waits for every attachment, says why, and the tray never claims more than the server said', async () => {
  const { fx } = await composerSite();
  const p = await openThread(browser, fx, 1);
  try {
    await type(p.page, 'Synthetic message with files');
    fx.api.hold('attachments');
    await addFiles(p.page, COMPOSER, [
      { name: 'notes.txt', buffer: text() },
      { name: 'photo.png', buffer: png(), mimeType: 'image/png' },
    ]);
    const t = await waitFor(async () => {
      const t = await readTray(p.page, COMPOSER);
      return t?.rows.length === 2 && t.rows.every((r) => r.state === 'uploading') ? t : null;
    }, { what: 'two uploads' });
    assert.deepEqual(t.rows.map((r) => r.word), ['uploading to your controller', 'uploading to your controller']);
    const c = await readComposer(p.page);
    assert.equal(c.off, 'can’t send: 2 attachments not ready');
    assert.equal((await sendButton(p.page)).disabled, true);
    // Pressing anyway sends nothing.
    await button(p.page, 'Send').click({ force: true });
    await p.page.keyboard.press('Control+Enter');
    assert.equal(posts(fx).length, 0, 'no send while attachments upload');
    // No word on the page says uploaded while they are on their way.
    const words = await p.page.locator(COMPOSER).innerText();
    assert.doesNotMatch(words, /\buploaded\b/i);
    fx.api.hold('attachments', false);
    fx.api.release('attachments');
    const ready = await trayIs(p.page, COMPOSER, 2, 'ready');
    assert.deepEqual(ready.rows.map((r) => r.word), ['ready on your controller', 'ready on your controller']);
    assert.equal(ready.metadata, 'sent unchanged, including any location data', 'B-10: the metadata note, with an image');
    assert.equal(ready.order, 'the agent gets files first, then images');
    await waitFor(async () => (await sendButton(p.page))?.disabled === false, { what: 'Send enabled' });
    await button(p.page, 'Send').click();
    await outcome(p.page, 'sent');
    const ids = [...fx.api.staged.keys()];
    assert.deepEqual(posts(fx)[0].attachments, ids, 'the ready ids, in the owner’s order');
    assert.deepEqual(fx.api.calls[0].attachments, ids);
    const after = await waitFor(async () => {
      const t = await readTray(p.page, COMPOSER);
      return t?.rows.length === 0 ? t : null;
    }, { what: 'the tray handed over' });
    assert.equal(after.note, '2 attachments handed to CloudRoom');
    assert.doesNotMatch(await p.page.locator(COMPOSER).innerText(), /delivered|\bread by\b|agent read/i);
  } finally {
    await p.close();
    await fx.close();
  }
});

test('AW5: a failed attachment keeps Send off until Retry or Remove; Remove of the failed one lets the rest go', async () => {
  const { fx } = await composerSite();
  let n = 0;
  const p = await openThread(browser, fx, 1);
  try {
    // The first upload fails with the core's reason; the second is ready.
    fx.api.setUpload((name, bytes) => {
      if (n++ === 0) {
        return { status: 200, headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ schema: 'openwiz.attachment-add.v1', ready: [], failed: [{ index: 0, path: name, readiness: { state: 'failed', reason: 'unsupported_heif' }, message: 'HEIC and HEIF images aren’t supported' }], not_staged: [], expired: [] }) };
      }
      const r = { id: `att-${'b'.repeat(32)}`, sha256: '0'.repeat(64), size_bytes: bytes.length, kind: 'file', name, display_name: name };
      fx.api.staged.set(r.id, r);
      return { status: 200, headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ schema: 'openwiz.attachment-add.v1', ready: [{ index: 0, path: name, staged: { attachment: r, readiness: { state: 'ready' }, ready_at: 1, age_ms: 0, expires_at: 2 } }], failed: [], not_staged: [], expired: [] }) };
    });
    n = 0;
    await type(p.page, 'Synthetic message');
    await addFiles(p.page, COMPOSER, [{ name: 'photo.heic', buffer: text('synthetic heic') }]);
    await addFiles(p.page, COMPOSER, [{ name: 'notes.txt', buffer: text() }]);
    const t = await waitFor(async () => {
      const t = await readTray(p.page, COMPOSER);
      return t?.rows.length === 2 && t.rows[0].state === 'failed' && t.rows[1].state === 'ready' ? t : null;
    }, { what: 'one failed, one ready' });
    assert.equal(t.rows[0].word, 'failed');
    assert.equal(t.rows[0].why, 'HEIC and HEIF images aren’t supported', 'the core’s words');
    assert.equal(t.rows[0].retry, true, 'Retry while the browser has the file');
    assert.equal((await readComposer(p.page)).off, 'can’t send: 1 attachment not ready');
    await p.page.locator(`${COMPOSER} [data-part=attachment-remove]`).first().click();
    await waitFor(async () => (await readTray(p.page, COMPOSER)).rows.length === 1, { what: 'the failed one removed' });
    await waitFor(async () => (await sendButton(p.page))?.disabled === false, { what: 'Send enabled' });
    assert.equal(fx.api.log.filter((e) => e.kind === 'attachment').length, 0, 'a failed one is never on the controller: no removal request');
    await button(p.page, 'Send').click();
    await outcome(p.page, 'sent');
    assert.deepEqual(posts(fx)[0].attachments, [`att-${'b'.repeat(32)}`]);
  } finally {
    await p.close();
    await fx.close();
  }
});

test('AW5: Steer is not offered with attachments; a line says why, and Queue carries them', async () => {
  const { fx } = await composerSite({ kinds: { [id(1)]: 'busy' } });
  const p = await openThread(browser, fx, 1);
  try {
    let c = await readComposer(p.page);
    assert.deepEqual(c.actions.map((a) => a.key), ['queue', 'steer']);
    await type(p.page, 'Synthetic message');
    await addFiles(p.page, COMPOSER, [{ name: 'notes.txt', buffer: text() }]);
    await trayIs(p.page, COMPOSER, 1, 'ready');
    c = await readComposer(p.page);
    assert.deepEqual(c.actions.map((a) => a.key), ['queue'], 'no Steer with attachments');
    assert.equal(await p.page.locator(`${COMPOSER} [data-part=no-steer]`).textContent(), 'Steer can’t carry attachments: Queue them, or remove them to Steer');
    await button(p.page, 'Queue').click();
    await waitFor(() => posts(fx).length === 1, { what: 'the queue' });
    assert.equal(posts(fx)[0].mode, 'queue');
    assert.equal(posts(fx)[0].attachments.length, 1);
  } finally {
    await p.close();
    await fx.close();
  }
});

test('AW5, AW6: in the launcher, Start thread waits for every attachment; the create carries them; the created thread says they were handed over', async () => {
  const { fx } = await launcherSite();
  const p = await openLauncher(browser, fx);
  try {
    await chooseAll(p.page);
    await typePrompt(p.page, 'Synthetic prompt with a file');
    fx.api.hold('attachments');
    await addFiles(p.page, LAUNCHER, [{ name: 'photo.png', buffer: png(), mimeType: 'image/png' }]);
    await waitFor(async () => (await readTray(p.page, LAUNCHER))?.rows[0]?.state === 'uploading', { what: 'uploading' });
    const l = await readLauncher(p.page);
    assert.equal(l.start.disabled, true);
    assert.match(l.startEffect, /can’t start: 1 attachment not ready/);
    assert.equal(createPosts(fx).length, 0);
    fx.api.hold('attachments', false);
    fx.api.release('attachments');
    await trayIs(p.page, LAUNCHER, 1, 'ready');
    const previewed = await waitFor(async () => {
      const bodies = fx.api.log.filter((e) => e.kind === 'create-preview' && e.body).map((e) => JSON.parse(e.body));
      return bodies.find((b) => b.attachments.length === 1) ?? null;
    }, { what: 'a preview with the attachment' });
    await waitFor(async () => (await readLauncher(p.page)).start?.disabled === false, { what: 'Start thread enabled' });
    await p.page.click('[data-part=launcher] [data-part=start]');
    const created = await settled(p.page, 'created');
    const ids = [...fx.api.staged.keys()];
    assert.deepEqual(createPosts(fx)[0].attachments, ids);
    assert.deepEqual(previewed.attachments, ids, 'the preview read exactly what the create sends');
    assert.equal(created.kind, 'created');
    assert.equal(await p.page.locator('[data-part=detail] [data-part=handed-over]').textContent(), '1 attachment handed to CloudRoom');
  } finally {
    await p.close();
    await fx.close();
  }
});
