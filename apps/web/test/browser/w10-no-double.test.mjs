// W10: no double send. Double clicks, a repeated keyboard send, client
// retries, reloads and a duplicated tab all reuse one op id, and make one
// provider call (the fake's `calls`, its stand-in for the core's `tell`).

import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';

import { launchBrowser, readView, waitFor } from '../support/browser.mjs';
import { apiError, DROP, HANG, json, LOST } from '../support/fake-api.mjs';
import { button, composerSite, id, openThread, outcome, posts, readComposer, select, TEXT, type } from '../support/composer.mjs';
import { DELIVERIES, outcome as result, report } from '../support/synthetic.ts';

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(() => browser.close());

const sent = (tid, b) => json(200, report(tid, b.op_id, b.mode, b.text, result(DELIVERIES.sent)));

test('a double click and a repeated keyboard send make one request and one call', async () => {
  const { fx } = await composerSite();
  fx.api.hold('send');
  const p = await openThread(browser, fx, 1);
  try {
    await type(p.page, TEXT);
    await button(p.page, 'Send').dblclick();
    await waitFor(() => fx.api.held('send') === 1, { what: 'the held send' });
    const box = p.page.locator('[data-part=composer] textarea');
    for (let i = 0; i < 5; i++) await box.press('Control+Enter');
    await p.page.keyboard.down('Control');
    for (let i = 0; i < 3; i++) await p.page.keyboard.down('Enter');
    await p.page.keyboard.up('Enter');
    await p.page.keyboard.up('Control');
    await p.page.clock.runFor(2_000);
    fx.api.release('send');
    await outcome(p.page, 'sent');
    assert.equal(posts(fx).length, 1, 'one request');
    assert.equal(fx.api.calls.length, 1, 'one call');
  } finally {
    await p.close();
    await fx.close();
  }
});

test('a client retry after a lost answer reuses the op id: the provider is called once', async () => {
  const { fx } = await composerSite();
  fx.api.setSend((tid, b) => ({ after: DROP, report: report(tid, b.op_id, b.mode, b.text, result(DELIVERIES.sent)) }));
  const p = await openThread(browser, fx, 1);
  try {
    await type(p.page, TEXT);
    await button(p.page, 'Send').click();
    await outcome(p.page, 'uncertain');
    await button(p.page, 'Try again').click();
    const c = await outcome(p.page, 'sent');
    const [a, b] = posts(fx);
    assert.equal(b.op_id, a.op_id, 'the retry carries the same op id');
    assert.equal(fx.api.calls.length, 1, 'the second request was a replay: no call');
    assert.match(c.lines[0], /sent/);
  } finally {
    await p.close();
    await fx.close();
  }
});

test('no answer in 45 s is uncertain, and nothing is resent on its own', async () => {
  const { fx } = await composerSite();
  fx.api.setSend(() => HANG);
  const p = await openThread(browser, fx, 1);
  try {
    await type(p.page, TEXT);
    await button(p.page, 'Send').click();
    await outcome(p.page, 'moving');
    await p.page.clock.runFor(44_000);
    assert.equal((await readComposer(p.page)).outcome, 'moving', 'still waiting before 45 s');
    await p.page.clock.runFor(1_500);
    const c = await outcome(p.page, 'uncertain');
    assert.match(c.lines.join(' '), /no answer within 45 s/);
    await p.page.clock.runFor(5 * 60_000);
    assert.equal(posts(fx).length, 1);
    assert.equal(fx.api.calls.length, 1);
  } finally {
    await p.close();
    await fx.close();
  }
});

test('a reload during the send keeps the op id and the text; Try again makes no second call', async () => {
  const { fx } = await composerSite();
  fx.api.hold('send');
  const p = await openThread(browser, fx, 1);
  try {
    await type(p.page, TEXT);
    await button(p.page, 'Send').click();
    await waitFor(() => fx.api.held('send') === 1, { what: 'the held send' });
    await p.page.reload();
    fx.api.release('send');
    await waitFor(() => fx.api.log.some((e) => e.kind === 'send' && e.endedAt !== null), { what: 'the first send to end' });
    await waitFor(async () => (await readView(p.page)).groups.length > 0, { what: 'rows after the reload' });
    await select(p.page, 1);
    let c = await outcome(p.page, 'uncertain');
    // Unloading aborts the request first, or the page is gone before it can say so: either way, uncertain.
    assert.match(c.lines.join(' '), /the page reloaded during the send|the connection was lost before an answer|not settled when this page loaded/);
    assert.equal(c.draft, TEXT, 'the text survived the reload');
    fx.api.hold('send', false);
    await button(p.page, 'Try again').click();
    c = await outcome(p.page, 'sent');
    const [a, b] = posts(fx);
    assert.equal(b.op_id, a.op_id);
    assert.equal(fx.api.calls.length, 1, 'one call across the reload');
  } finally {
    await p.close();
    await fx.close();
  }
});

test('a duplicated tab carries the same op id: two tabs, one call', async () => {
  const { fx } = await composerSite();
  fx.api.setSend((tid, b) => ({ after: DROP, report: report(tid, b.op_id, b.mode, b.text, result(DELIVERIES.sent)) }));
  const p = await openThread(browser, fx, 1);
  try {
    await type(p.page, TEXT);
    await button(p.page, 'Send').click();
    await outcome(p.page, 'uncertain');
    // A duplicated tab starts with a copy of the tab's sessionStorage.
    const storage = await p.page.evaluate(() => JSON.stringify(Object.entries(sessionStorage)));
    const second = await p.context.newPage();
    await second.addInitScript((entries) => {
      if (sessionStorage.length === 0) for (const [k, v] of JSON.parse(entries)) sessionStorage.setItem(k, v);
    }, storage);
    await second.clock.install({ time: new Date(Date.now()) });
    await second.goto(fx.site.origin);
    await waitFor(async () => (await readView(second)).groups.length > 0, { what: 'rows in the second tab' });
    await select(second, 1);
    await outcome(second, 'uncertain');
    await button(second, 'Try again').click();
    await button(p.page, 'Try again').click();
    await outcome(second, 'sent');
    await outcome(p.page, 'sent');
    const ops = new Set(posts(fx).map((b) => b.op_id));
    assert.equal(ops.size, 1, 'one op id in both tabs');
    assert.equal(fx.api.calls.length, 1, 'one call');
    assert.equal(fx.api.calls[0].id, id(1));
  } finally {
    await p.close();
    await fx.close();
  }
});



/** Opens a second tab in `p`'s context with a copy of `p`'s sessionStorage, as a duplicated tab has. */
async function duplicate(p, fx) {
  const storage = await p.page.evaluate(() => JSON.stringify(Object.entries(sessionStorage)));
  const second = await p.context.newPage();
  await second.addInitScript((entries) => {
    if (sessionStorage.length === 0) for (const [k, v] of JSON.parse(entries)) sessionStorage.setItem(k, v);
  }, storage);
  await second.clock.install({ time: new Date(Date.now()) });
  await second.goto(fx.site.origin);
  await waitFor(async () => (await readView(second)).groups.length > 0, { what: 'rows in the second tab' });
  await select(second, 1);
  return second;
}

test('a tab duplicated before the send carries the draft’s op id: send in both, one call', async () => {
  const { fx } = await composerSite();
  const p = await openThread(browser, fx, 1);
  try {
    await type(p.page, TEXT);
    const second = await duplicate(p, fx);
    assert.equal((await readComposer(second)).draft, TEXT, 'the copy has the unsent draft');
    await button(p.page, 'Send').click();
    await outcome(p.page, 'sent');
    await button(second, 'Send').click();
    await outcome(second, 'sent');
    const ops = posts(fx).map((b) => b.op_id);
    assert.equal(ops.length, 2);
    assert.equal(ops[1], ops[0], 'one op id in both tabs');
    assert.equal(fx.api.calls.length, 1, 'the second was a replay: one call');
  } finally {
    await p.close();
    await fx.close();
  }
});

test('a lost send, then a retry the core refuses as a possible duplicate, stays uncertain with its op id', async () => {
  const { fx } = await composerSite();
  let n = 0;
  const earlier = [{ op_id: `web-${'e'.repeat(32)}`, mode: 'queue', claimed_at: 1, in_flight: true, reconciliation: { kind: 'not_read' } }];
  fx.api.setSend((tid, b) => (n++ === 0 ? LOST : json(200, report(tid, b.op_id, b.mode, b.text, { kind: 'possible_duplicate_refused', earlier }))));
  const p = await openThread(browser, fx, 1);
  try {
    await type(p.page, TEXT);
    await button(p.page, 'Send').click();
    await outcome(p.page, 'uncertain');
    await button(p.page, 'Try again').click();
    await waitFor(() => posts(fx).length === 2 && fx.api.log.filter((e) => e.kind === 'send').every((e) => e.endedAt !== null), { what: 'the retry' });
    const c = await outcome(p.page, 'uncertain');
    assert.equal(c.word, 'uncertain', 'never "not sent": the refusal speaks for the retry only');
    assert.match(c.lines.join(' '), /the earlier send is still uncertain/);
    assert.equal(c.draft, TEXT);
    assert.equal(c.readOnly, true);
    const [a, b] = posts(fx);
    assert.equal(b.op_id, a.op_id);
    assert.deepEqual(c.unsettled.slice(0, 2), ['Check again', 'Try again'], 'the same op id can still be checked and retried');
  } finally {
    await p.close();
    await fx.close();
  }
});

/** Makes this page's sessionStorage writes fail (a full quota) from when `window.__full` is set, or always. */
const quota = (always) => (flag) => {
  const set = Storage.prototype.setItem;
  Storage.prototype.setItem = function (k, v) {
    if (flag || window.__full) throw new DOMException('quota', 'QuotaExceededError');
    return set.call(this, k, v);
  };
};

test('when the op id can’t be stored, nothing is sent, and the composer says why', async () => {
  const { fx } = await composerSite();
  const p = await openThread(browser, fx, 1);
  try {
    await p.page.evaluate(quota(false), false);
    await type(p.page, TEXT);

    await p.page.evaluate(() => {
      window.__full = true;
    });
    await button(p.page, 'Send').click();
    await p.page.clock.runFor(2_000);
    assert.equal(posts(fx).length, 0, 'no POST without a stored binding');
    let c = await readComposer(p.page);
    assert.match(c.off ?? '', /can’t send: .*can’t be saved/);
    assert.ok(c.actions.every((a) => a.disabled));
    assert.equal(c.draft, TEXT, 'the draft stays');
    // An edit that can't be saved keeps sending off too; once storage works again, sending is back.
    await type(p.page, `${TEXT} edited`);
    await p.page.locator('[data-part=composer] textarea').press('Control+Enter');
    assert.equal(posts(fx).length, 0);
    await p.page.evaluate(() => {
      window.__full = false;
    });
    await type(p.page, TEXT);
    c = await readComposer(p.page);
    assert.equal(c.off, null);
    await button(p.page, 'Send').click();
    await outcome(p.page, 'sent');
    assert.equal(posts(fx).length, 1);
  } finally {
    await p.close();
    await fx.close();
  }
});

test('with storage refused from the start, sending is disabled with a reason', async () => {
  const { fx } = await composerSite();
  const p = await openThread(browser, fx, 1);
  try {
    await p.page.addInitScript(quota(true), true);
    await p.page.reload();
    await waitFor(async () => (await readView(p.page)).groups.length > 0, { what: 'rows' });
    await select(p.page, 1);
    await type(p.page, TEXT);
    const c = await readComposer(p.page);
    assert.match(c.off ?? '', /can’t send: .*can’t be saved/);
    await p.page.locator('[data-part=composer] textarea').press('Control+Enter');
    await p.page.clock.runFor(1_000);
    assert.equal(posts(fx).length, 0);
  } finally {
    await p.close();
    await fx.close();
  }
});

test('storage holds the draft, its root, the next numbers and the unresolved attempts only; nothing once sent', async () => {
  const { fx } = await composerSite();
  fx.api.hold('send');
  const p = await openThread(browser, fx, 1);
  const storage = () => p.page.evaluate(() => Object.fromEntries(Object.entries(sessionStorage)));
  try {
    await type(p.page, TEXT);
    await button(p.page, 'Send').click();
    await waitFor(() => fx.api.held('send') === 1, { what: 'the held send' });
    const during = await storage();
    assert.deepEqual(Object.keys(during), [`openwiz.composer.${id(1)}`]);
    assert.deepEqual(Object.keys(JSON.parse(Object.values(during)[0])).sort(), ['attempts', 'draft', 'next', 'root']);
    fx.api.release('send');
    await outcome(p.page, 'sent');
    assert.deepEqual(await storage(), {}, 'nothing is left: no report, title, hash or observation');
  } finally {
    await p.close();
    await fx.close();
  }
});



const text = (page) => page.locator('[data-part=composer]').textContent();
const fail503 = () => apiError(503, 'busy', 'no core run slot within its wait', { 'Retry-After': '5' });
const uncertainReport = (tid, b) => json(200, report(tid, b.op_id, b.mode, b.text, result(DELIVERIES.uncertain)));

test('1: a check in one tab and Send again in its copy share one successor: two calls in all, not three', async () => {
  const { fx } = await composerSite();
  let n = 0;
  fx.api.setSend((tid, b) => (n++ === 0 ? uncertainReport(tid, b) : sent(tid, b)));
  const p = await openThread(browser, fx, 1);
  try {
    await type(p.page, TEXT);
    await button(p.page, 'Send').click();
    await outcome(p.page, 'uncertain');
    const second = await duplicate(p, fx);
    await outcome(second, 'uncertain');
    await button(p.page, 'Check again').click();
    await waitFor(() => fx.api.log.some((e) => e.kind === 'lookup' && e.completed), { what: 'the check' });
    await button(second, 'Send again').click();
    await waitFor(() => fx.api.calls.length === 2, { what: 'the second tab’s send' });
    await button(p.page, 'Send again').click();
    await waitFor(() => posts(fx).length === 3 && fx.api.log.filter((e) => e.kind === 'send').every((e) => e.endedAt !== null), { what: 'the first tab’s send' });
    const ops = posts(fx).map((b) => b.op_id);
    assert.equal(ops[2], ops[1], 'the first tab resends the same successor');
    assert.equal(new Set(ops).size, 2);
    assert.equal(fx.api.calls.length, 2, 'the original call plus one shared resend');
  } finally {
    await p.close();
    await fx.close();
  }
});

test('2: a failed Send again is shown as its own outcome; the earlier send stays uncertain, with Check again', async () => {
  const { fx } = await composerSite();
  let n = 0;
  fx.api.setSend((tid, b) =>
    n++ === 0 ? uncertainReport(tid, b) : json(200, report(tid, b.op_id, b.mode, b.text, result({ kind: 'failed', reason: 'target_archived', text: 'failed: the target is archived, and archived targets are refused' }))),
  );
  const p = await openThread(browser, fx, 1);
  try {
    await type(p.page, TEXT);
    await button(p.page, 'Send').click();
    await outcome(p.page, 'uncertain');
    await button(p.page, 'Send again').click();
    await waitFor(async () => (await text(p.page)).includes('target is archived'), { what: 'the failure' });
    const c = await readComposer(p.page);
    assert.deepEqual(c.rows.map((r) => r.outcome), ['uncertain', 'failed'], 'the earlier send is still uncertain; the new one failed');
    assert.equal(c.readOnly, true, 'its text is still fixed');
    assert.ok(c.unsettled.includes('Check again'), 'with its lookup');
    await p.page.reload();
    await waitFor(async () => (await readView(p.page)).groups.length > 0, { what: 'rows' });
    await select(p.page, 1);
    assert.deepEqual((await readComposer(p.page)).rows.map((r) => r.outcome), ['uncertain', 'failed'], 'and after a reload');
    await button(p.page, 'Check again').click();
    await waitFor(() => fx.api.log.some((e) => e.kind === 'lookup'), { what: 'the check' });
    assert.equal(fx.api.log.find((e) => e.kind === 'lookup').path, `/api/v1/sessions/${id(1)}/messages/${posts(fx)[0].op_id}`);
  } finally {
    await p.close();
    await fx.close();
  }
});

test('3: Try again, Send again and Send anyway need a usable preview, like Send', async () => {
  const cases = [
    ['Try again', (tid, b) => LOST, 'uncertain'],
    ['Send again', (tid, b) => uncertainReport(tid, b), 'uncertain'],
    [
      'Send anyway',
      (tid, b) => json(200, report(tid, b.op_id, b.mode, b.text, { kind: 'possible_duplicate_refused', earlier: [{ op_id: `web-${'e'.repeat(32)}`, mode: 'queue', claimed_at: 1, in_flight: true, reconciliation: { kind: 'not_read' } }] })),
      'not-sent',
    ],
  ];
  for (const [label, answer, shape] of cases) {
    const { fx } = await composerSite();
    fx.api.setSend(answer);
    const p = await openThread(browser, fx, 1);
    try {
      await type(p.page, TEXT);
      await button(p.page, 'Send').click();
      await outcome(p.page, shape);
      fx.api.setPreview(fail503);
      await p.page.clock.runFor(15_000);
      await waitFor(async () => (await text(p.page)).includes('couldn’t be read'), { what: `${label}: the preview failure` });
      assert.equal(await button(p.page, label).isDisabled(), true, `${label} is disabled without a preview`);
      await button(p.page, label).click({ force: true });
      await p.page.clock.runFor(1_000);
      assert.equal(posts(fx).length, 1, `${label}: no POST`);
    } finally {
      await p.close();
      await fx.close();
    }
  }
});

test('4: when storage fails while a send is unsettled, the disabled controls say why', async () => {
  const { fx } = await composerSite();
  fx.api.setSend((tid, b) => ({ after: DROP, report: report(tid, b.op_id, b.mode, b.text, result(DELIVERIES.sent)) }));
  const p = await openThread(browser, fx, 1);
  try {
    await p.page.evaluate(quota(false), false);
    await type(p.page, TEXT);
    await button(p.page, 'Send').click();
    await outcome(p.page, 'uncertain');
    await p.page.evaluate(() => {
      window.__full = true;
    });
    await button(p.page, 'Try again').click();
    await p.page.clock.runFor(1_000);
    assert.equal(posts(fx).length, 1, 'no POST');
    assert.match(await text(p.page), /can’t send: the message id can’t be saved in this tab/);
    assert.equal(await button(p.page, 'Try again').isDisabled(), true);
  } finally {
    await p.close();
    await fx.close();
  }
});



test('2: after a lost acknowledgement, Forget it in copied tabs: both send the same next attempt, never the forgotten one', async () => {
  const { fx } = await composerSite();
  let n = 0;
  fx.api.setSend((tid, b) => (n++ === 0 ? { after: DROP, report: report(tid, b.op_id, b.mode, b.text, result(DELIVERIES.sent)) } : sent(tid, b)));
  const p = await openThread(browser, fx, 1);
  try {
    await type(p.page, TEXT);
    await button(p.page, 'Send').click();
    await outcome(p.page, 'uncertain');
    const second = await duplicate(p, fx);
    for (const page of [p.page, second]) {
      await button(page, 'Forget it').click();
      await button(page, 'Send').click();
      await waitFor(() => fx.api.log.filter((e) => e.kind === 'send' && e.endedAt !== null).length === (page === p.page ? 2 : 3), { what: 'the send' });
    }
    const [a, b, c] = posts(fx).map((x) => x.op_id);
    assert.notEqual(b, a, 'never the forgotten op id');
    assert.equal(c, b, 'both tabs send the same next attempt');
    assert.equal(fx.api.calls.length, 2, 'the original call, and one for the next attempt');
  } finally {
    await p.close();
    await fx.close();
  }
});



/** A uncertain, then focus on Send again; B is answered by `second`. */
async function heldSetup(second) {
  const { fx } = await composerSite();
  let n = 0;
  fx.api.setSend((tid, b) => (n++ === 0 ? uncertainReport(tid, b) : second(tid, b)));
  const p = await openThread(browser, fx, 1);
  await type(p.page, TEXT);
  await button(p.page, 'Send').click();
  await outcome(p.page, 'uncertain');
  await button(p.page, 'Send again').focus();
  return { fx, p };
}
const answered = (fx, n) => waitFor(() => fx.api.log.filter((e) => e.kind === 'send' && e.endedAt !== null).length === n, { what: `${n} answered sends` });

test('holding Enter on Send again across the answer sends once: repeats never activate it again', async () => {
  const { fx, p } = await heldSetup(uncertainReport);
  try {
    await p.page.keyboard.down('Enter');
    await answered(fx, 2);
    await waitFor(async () => (await readComposer(p.page)).rows.length === 2 && (await readComposer(p.page)).outcome !== 'moving', { what: 'B answered' });
    for (let i = 0; i < 8; i++) await p.page.keyboard.down('Enter'); // auto-repeat: repeat true
    await p.page.clock.runFor(2_000);
    assert.equal(posts(fx).length, 2, 'A and one resend, B');
    assert.equal(fx.api.calls.length, 2);
    await p.page.keyboard.up('Enter');
    // A fresh press is a new, deliberate resend.
    await p.page.keyboard.press('Enter');
    await answered(fx, 3);
    assert.equal(fx.api.calls.length, 3);
    assert.equal(await p.page.evaluate(() => document.activeElement?.getAttribute('data-part')), 'send-again', 'focus stays');
  } finally {
    await p.close();
    await fx.close();
  }
});

test('holding Space on Send again (it activates on release) sends once', async () => {
  const { fx, p } = await heldSetup(uncertainReport);
  try {
    await p.page.keyboard.down(' ');
    for (let i = 0; i < 8; i++) await p.page.keyboard.down(' ');
    await p.page.clock.runFor(1_000);
    assert.equal(posts(fx).length, 1, 'nothing while held');
    await p.page.keyboard.up(' ');
    await answered(fx, 2);
    await p.page.clock.runFor(2_000);
    assert.equal(posts(fx).length, 2, 'one resend on release');
    assert.equal(fx.api.calls.length, 2);
  } finally {
    await p.close();
    await fx.close();
  }
});

test('a click that arrives while Enter is still held (an auto-repeat that clicks) is not a second press', async () => {
  const { fx, p } = await heldSetup(uncertainReport);
  try {
    await p.page.keyboard.down('Enter');
    await answered(fx, 2);
    await waitFor(async () => (await readComposer(p.page)).outcome === 'uncertain' && (await readComposer(p.page)).rows.length === 2, { what: 'B answered' });
    await p.page.evaluate(() => document.querySelector('[data-part=send-again]').click());
    await p.page.clock.runFor(2_000);
    assert.equal(posts(fx).length, 2, 'the held key’s press was spent on B');
    await p.page.keyboard.up('Enter');
    await p.page.evaluate(() => document.querySelector('[data-part=send-again]').click());
    await answered(fx, 3);
    assert.equal(posts(fx).length, 3, 'a click with no key held (assistive technology) is a fresh activation');
  } finally {
    await p.close();
    await fx.close();
  }
});
