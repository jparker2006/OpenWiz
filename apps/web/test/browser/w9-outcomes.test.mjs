// W9: the requested mode and the reported delivery stay separate, and
// queued, sent, failed, unsupported and uncertain each look distinct: a
// word, a glyph and a shape, told apart in greyscale. Uncertain keeps the
// text, and stays uncertain on a text-and-time match: Check again only reads.

import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';

import { DESKTOP, launchBrowser, PHONE, waitFor } from '../support/browser.mjs';
import { apiError, DROP, json, LOST } from '../support/fake-api.mjs';
import { button, composerSite, id, openThread, outcome, posts, readComposer, TEXT, type } from '../support/composer.mjs';
import { decodePng, inkMask, maskDifference } from '../support/png.mjs';
import { DELIVERIES, outcome as result, report } from '../support/synthetic.ts';
/** V2's threshold: shapes that differ by fewer pixels look the same. */
const DISTINCT_PIXELS = 20;

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(() => browser.close());

const answers = {
  queued: (tid, b) => json(200, report(tid, b.op_id, b.mode, b.text, result(DELIVERIES.queuedInteraction))),
  sent: (tid, b) => json(200, report(tid, b.op_id, b.mode, b.text, result(DELIVERIES.sent))),
  failed: (tid, b) => json(200, report(tid, b.op_id, b.mode, b.text, result(DELIVERIES.failed))),
  unsupported: (tid, b) => json(200, report(tid, b.op_id, b.mode, b.text, result(DELIVERIES.unsupported))),
  uncertain: (tid, b) => json(200, report(tid, b.op_id, b.mode, b.text, result(DELIVERIES.uncertainMatched))),
  'not-sent': () => apiError(503, 'busy', 'no send slot within its wait'),
};

/** Sends once on a busy thread with Steer, answered by `kind`; returns the page in that outcome. */
async function sentAs(kind, viewport = DESKTOP) {
  const { fx } = await composerSite({ kinds: { [id(1)]: 'busy' } });
  fx.api.setSend(answers[kind]);
  const p = await openThread(browser, fx, 1, { viewport });
  await type(p.page, TEXT);
  await button(p.page, 'Steer').click();
  const c = await outcome(p.page, kind);
  return { fx, p, c };
}

test('the requested mode and the reported delivery are shown apart, in the core’s words', async () => {
  const want = {
    queued: ['queued', 'Steer requested · queued: waiting on an interaction'],
    sent: ['sent', 'Steer requested · sent: the provider accepted the message, not that the work completed'],
    failed: ['failed', 'Steer requested · failed: the provider rejected the input'],
    unsupported: ['unsupported', 'Steer requested · failed: unsupported mode: the provider does not support the requested mode for this thread; no other mode was tried'],
    uncertain: ['uncertain', 'Steer requested · uncertain: the provider did not answer in time. The message may or may not have been accepted; the text is kept and nothing is resent'],
    'not-sent': ['not sent', 'busy: no send slot within its wait'],
  };
  for (const [kind, [word, line]] of Object.entries(want)) {
    const { fx, p, c } = await sentAs(kind);
    try {
      assert.equal(c.word, word, `${kind}: the delivery's word`);
      assert.equal(c.lines[0], line, `${kind}: the summary`);
      assert.notEqual(c.word, 'steer', 'the requested mode is never shown as the delivery');
      const kept = !['queued', 'sent'].includes(kind);
      assert.equal(c.draft, kept ? TEXT : '', `${kind}: the text ${kept ? 'stays' : 'is gone'}`);
    } finally {
      await p.close();
      await fx.close();
    }
  }
});

test('in greyscale, every outcome differs from every other by glyph and shape alone', async () => {
  for (const viewport of [PHONE, DESKTOP]) {
    const masks = {};
    for (const kind of ['queued', 'sent', 'failed', 'unsupported', 'uncertain', 'not-sent']) {
      const { fx, p } = await sentAs(kind, viewport);
      try {
        await p.page.evaluate(() => {
          document.documentElement.style.filter = 'grayscale(1)';
          document.querySelector('[data-part=outcome-marker] .word').textContent = 'outcome';
        });
        const marker = p.page.locator('[data-part=outcome-marker]');
        await marker.scrollIntoViewIfNeeded();
        const box = await marker.boundingBox();
        const png = await p.page.screenshot({ clip: { x: box.x - 10, y: box.y - 4, width: box.width + 20, height: box.height + 8 } });
        masks[kind] = inkMask(decodePng(png));
      } finally {
        await p.close();
        await fx.close();
      }
    }
    const kinds = Object.keys(masks);
    const diffs = {};
    for (let i = 0; i < kinds.length; i++) {
      for (let j = i + 1; j < kinds.length; j++) {
        const d = maskDifference(masks[kinds[i]], masks[kinds[j]]);
        diffs[`${kinds[i]}-${kinds[j]}`] = d;
        assert.ok(d >= DISTINCT_PIXELS, `${kinds[i]} and ${kinds[j]} look the same in greyscale (${d} px differ)`);
      }
    }
    console.log(`outcome greyscale mask differences at ${viewport.width}px:`, JSON.stringify(diffs));
  }
});

test('uncertain keeps the text and the op id; Check again only reads, and a text-and-time match stays uncertain', async () => {
  const { fx, p } = await sentAs('uncertain');
  try {
    let c = await readComposer(p.page);
    assert.equal(c.readOnly, true, 'the text is fixed while the send is uncertain');
    assert.ok(c.lines.includes('observed 1 matching of 3'), 'the observation is shown');
    assert.ok(c.lines.some((l) => l.startsWith('none: room-cli 0.43.1 documents no identifier')), 'with the attribution gap');
    assert.deepEqual(c.unsettled, ['Check again', 'Send again', 'Forget it']);
    const sent = posts(fx);
    await button(p.page, 'Check again').click();
    await waitFor(() => fx.api.log.some((e) => e.kind === 'lookup' && e.completed), { what: 'the lookup' });
    c = await outcome(p.page, 'uncertain');
    assert.equal(c.word, 'uncertain', 'still uncertain after the check');
    assert.equal(c.draft, TEXT);
    const lookups = fx.api.log.filter((e) => e.kind === 'lookup');
    assert.equal(lookups.length, 1);
    assert.equal(lookups[0].method, 'GET');
    assert.equal(lookups[0].path, `/api/v1/sessions/${id(1)}/messages/${sent[0].op_id}`, 'the same op id');
    assert.equal(posts(fx).length, 1, 'Check again never sends');
    assert.equal(fx.api.calls.length, 1);
    // Send again is an explicit new send that says it may duplicate.
    fx.api.setSend(answers.sent);
    await button(p.page, 'Send again').click();
    const both = await waitFor(async () => {
      const r = await readComposer(p.page);
      return r.rows[1]?.outcome === 'sent' ? r : null;
    }, { what: 'the second send’s outcome' });
    // The new send settles only itself: the earlier one stays uncertain, shown first, with its lookup.
    assert.equal(both.rows[0].outcome, 'uncertain');
    assert.ok(both.unsettled.includes('Check again'));
    const [first, second] = posts(fx);
    assert.notEqual(second.op_id, first.op_id);
    assert.equal(second.accept_possible_duplicate, true);
  } finally {
    await p.close();
    await fx.close();
  }
});

test('a lost answer is uncertain, never "not sent", and is never resent by itself', async () => {
  const { fx } = await composerSite();
  // The provider was called, but the connection dropped before the answer.
  fx.api.setSend((tid, b) => ({ after: DROP, report: report(tid, b.op_id, b.mode, b.text, result(DELIVERIES.sent)) }));
  const p = await openThread(browser, fx, 1);
  try {
    await type(p.page, TEXT);
    await button(p.page, 'Send').click();
    const c = await outcome(p.page, 'uncertain');
    assert.equal(c.word, 'uncertain');
    assert.match(c.lines.join(' '), /may or may not have been sent/);
    assert.equal(c.draft, TEXT);
    await p.page.clock.runFor(60_000);
    assert.equal(posts(fx).length, 1, 'no resend on its own');
    // Check again finds the recorded outcome: sent.
    await button(p.page, 'Check again').click();
    const done = await outcome(p.page, 'sent');
    assert.equal(done.draft, '');
    assert.equal(fx.api.calls.length, 1);
  } finally {
    await p.close();
    await fx.close();
  }
});



const words = (page) => page.$$eval('[data-part=composer] [data-part=outcome-marker] .word', (els) => els.map((e) => e.textContent));

/** A uncertain, B uncertain, then C answered by `last`. */
async function threeAttempts(last) {
  const { fx } = await composerSite();
  let n = 0;
  fx.api.setSend((tid, b) => (n++ < 2 ? answers.uncertain(tid, b) : answers[last](tid, b)));
  const p = await openThread(browser, fx, 1);
  await type(p.page, TEXT);
  await button(p.page, 'Send').click();
  await waitFor(async () => (await words(p.page)).includes('uncertain'), { what: 'A uncertain' });
  for (let i = 2; i <= 3; i++) {
    await button(p.page, 'Send again').click();
    await waitFor(() => fx.api.log.filter((e) => e.kind === 'send' && e.endedAt !== null).length === i, { what: `attempt ${i}` });
  }
  return { fx, p };
}

for (const last of ['sent', 'failed']) {
  test(`1: a third attempt’s outcome (${last}) is shown, with both earlier uncertain attempts`, async () => {
    const { fx, p } = await threeAttempts(last);
    try {
      const shown = await waitFor(async () => {
        const w = await words(p.page);
        return w.includes(last) ? w : null;
      }, { what: `C ${last}`, timeout: 5_000 });
      assert.deepEqual(shown, ['uncertain', 'uncertain', last], 'every attempt, in order');
      assert.equal(posts(fx).length, 3);
    } finally {
      await p.close();
      await fx.close();
    }
  });
}

test('3: a later attempt’s outcome is inside the live region; focus stays put', async () => {
  const { fx } = await composerSite();
  let n = 0;
  fx.api.setSend((tid, b) => (n++ === 0 ? answers.uncertain(tid, b) : answers.failed(tid, b)));
  const p = await openThread(browser, fx, 1);
  try {
    await type(p.page, TEXT);
    await button(p.page, 'Send').click();
    await waitFor(async () => (await words(p.page)).includes('uncertain'), { what: 'A uncertain' });
    await button(p.page, 'Send again').focus();
    await p.page.keyboard.press('Enter');
    await waitFor(async () => (await words(p.page)).includes('failed'), { what: 'B failed', timeout: 5_000 });
    const live = await p.page.evaluate(() => {
      const failed = [...document.querySelectorAll('[data-part=composer] [data-part=outcome-marker]')].find((m) => m.textContent.includes('failed'));
      const region = failed?.closest('[aria-live=polite], [role=status]');
      return { inRegion: !!region, focus: document.activeElement?.getAttribute('data-part') ?? document.activeElement?.tagName ?? null };
    });
    assert.equal(live.inRegion, true, 'B’s failure is announced');
    assert.equal(live.focus, 'send-again', 'focus stays on the pressed button ');
  } finally {
    await p.close();
    await fx.close();
  }
});

test('4: Try again and Send again show their preview’s effect and age, while the text is fixed', async () => {
  for (const [answer, label] of [
    [() => LOST, 'Try again'],
    [answers.uncertain, 'Send again'],
  ]) {
    const { fx } = await composerSite();
    fx.api.setSend(answer);
    const p = await openThread(browser, fx, 1);
    try {
      await type(p.page, TEXT);
      await button(p.page, 'Send').click();
      await waitFor(async () => (await words(p.page)).includes('uncertain'), { what: 'uncertain' });
      const row = await p.page.evaluate((l) => {
        const b = [...document.querySelectorAll('[data-part=composer] button')].find((x) => x.textContent === l);
        const box = b?.closest('[data-send]') ?? b?.parentElement?.parentElement;
        return box ? box.textContent : null;
      }, label);
      assert.ok(row, `${label} is there`);
      assert.match(row, /starts a turn; may wake it/, `${label}: the effect`);
      assert.match(row, /effect read \d+s ago/, `${label}: the read’s age`);
    } finally {
      await p.close();
      await fx.close();
    }
  }
});



const focused = (page) =>
  page.evaluate(() => {
    const a = document.activeElement;
    return a?.getAttribute('data-part') ?? (a?.id ? `#${a.id}` : (a?.tagName ?? null));
  });

test('Send again by keyboard keeps focus on it while the request is out, and after', async () => {
  const { fx } = await composerSite();
  let n = 0;
  fx.api.setSend((tid, b) => (n++ === 0 ? answers.uncertain(tid, b) : answers.failed(tid, b)));
  const p = await openThread(browser, fx, 1);
  try {
    await type(p.page, TEXT);
    await button(p.page, 'Send').click();
    await waitFor(async () => (await words(p.page)).includes('uncertain'), { what: 'A uncertain' });
    fx.api.hold('send');
    await button(p.page, 'Send again').focus();
    await p.page.keyboard.press('Enter');
    await waitFor(() => fx.api.held('send') === 1, { what: 'B held' });
    assert.equal(await focused(p.page), 'send-again', 'while waiting');
    fx.api.hold('send', false);
    fx.api.release('send');
    await waitFor(async () => (await words(p.page)).includes('failed'), { what: 'B failed' });
    assert.equal(await focused(p.page), 'send-again', 'after the answer');
  } finally {
    await p.close();
    await fx.close();
  }
});

test('Check again by keyboard keeps focus while it reads; when its row settles, focus goes to the message field', async () => {
  const { fx } = await composerSite();
  fx.api.setSend((tid, b) => ({ after: DROP, report: report(tid, b.op_id, b.mode, b.text, result(DELIVERIES.sent)) }));
  const p = await openThread(browser, fx, 1);
  try {
    await type(p.page, TEXT);
    await button(p.page, 'Send').click();
    await waitFor(async () => (await words(p.page)).includes('uncertain'), { what: 'uncertain' });
    fx.api.hold('lookup');
    await button(p.page, 'Check again').focus();
    await p.page.keyboard.press('Enter');
    await waitFor(() => fx.api.held('lookup') === 1, { what: 'the held lookup' });
    assert.equal(await focused(p.page), 'check', 'while reading');
    fx.api.hold('lookup', false);
    fx.api.release('lookup');
    await waitFor(async () => (await words(p.page)).includes('sent'), { what: 'the record' });
    assert.equal(await focused(p.page), 'draft', 'a fixed, named place: the message field');
  } finally {
    await p.close();
    await fx.close();
  }
});

test('each preview has its own age: Queue’s keeps counting while Steer’s read is held', async () => {
  // Deterministic (CI round): every step waits on an explicit state, and no answer can land on its own
  // between the clock jump and the reading. Steer's preview is held; then every detail and list read is
  // held too, so no refresh can start a second preview cycle (one landing just after the release did,
  // and re-stamped Queue's time: the race CI hit). Exactly one cycle is asserted: Queue, then Steer.
  const { fx } = await composerSite({ kinds: { [id(1)]: 'busy' } });
  fx.api.holdWhen((e) => e.kind === 'preview' && e.search.includes('mode=steer'));
  const { openPage, readView } = await import('../support/browser.mjs');
  const { START } = await import('../support/composer.mjs');
  const p = await openPage(browser, fx.site.origin, { clock: START, viewport: DESKTOP });
  const previews = () => fx.api.log.filter((e) => e.kind === 'preview');
  const state = () => p.page.locator('[data-part=composer]').getAttribute('data-preview');
  try {
    await waitFor(async () => (await readView(p.page)).groups.length > 0, { what: 'rows' });
    await p.page.click(`main [data-session-id="${id(1)}"]`);
    // Queue's preview has answered (the Steer read starts only after it), and Steer's is held.
    await waitFor(() => fx.api.held('preview') === 1 && previews().length === 2 && previews()[0].completed, { what: 'Queue answered, Steer held' });
    assert.equal(await state(), 'loading');
    fx.api.hold('detail');
    fx.api.hold('list');
    await p.page.clock.runFor(30_000);
    assert.equal(await state(), 'loading', 'still waiting for Steer after the jump');
    fx.api.holdWhen(() => false);
    fx.api.release('preview');
    await waitFor(async () => (await state()) === 'ready', { what: 'both previews' });
    const c = await readComposer(p.page);
    const ages = await p.page.$$eval('[data-part=composer] [data-part=effect-age]', (els) => els.map((e) => e.textContent));
    assert.equal(previews().length, 2, 'one preview cycle: these ages are its own');
    assert.match(ages[0], /^effect read (3\d|4\d)s ago/, `Queue's read is over 30 s old: ${ages[0]}`);
    assert.match(ages[1], /^effect read [0-9]s ago/, `Steer's read is fresh: ${ages[1]}`);
    assert.equal(c.actions[0].label, 'Queue');
  } finally {
    fx.api.hold('detail', false);
    fx.api.hold('list', false);
    await p.close();
    await fx.close();
  }
});
