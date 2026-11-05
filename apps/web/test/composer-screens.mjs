// Renders DEMO-labelled screenshots of the composer's states  at
// 390 and 1440 px, on the fake API: `node test/composer-screens.mjs [out-dir]`.
// Not a proof. Synthetic data only.

import { mkdirSync } from 'node:fs';

import { TIMING } from '../src/timing.ts';
import { DESKTOP, launchBrowser, PHONE, waitFor } from './support/browser.mjs';
import { button, composerSite, id, openThread, outcome, readComposer, TEXT, type } from './support/composer.mjs';
import { apiError, DROP, json, LOST } from './support/fake-api.mjs';
import { DELIVERIES, inventory, outcome as result, report, session } from './support/synthetic.ts';
import { RESULTS_DIR } from './support/paths.mjs';

const dir = process.argv[2] ?? `${RESULTS_DIR}/composer-screens`;
mkdirSync(dir, { recursive: true });

/** A DEMO ribbon, added outside the page's CSP, so no screenshot passes for real data. */
const demo = (page) =>
  page.evaluate(() => {
    const r = document.createElement('div');
    r.textContent = 'DEMO · SYNTHETIC DATA · NOTHING IS LIVE';
    Object.assign(r.style, {
      position: 'fixed', left: '0', right: '0', bottom: '0', zIndex: '100', padding: '4px 16px', textAlign: 'center',
      background: '#552583', color: '#e8eaed', font: '600 10px/16px GeistMono, monospace', letterSpacing: '.08em',
    });
    // Inside a modal sheet, which sits above everything else on the page.
    (document.querySelector('dialog[open]:modal') ?? document.body).append(r);
  });

const answer = (d) => (tid, b) => json(200, report(tid, b.op_id, b.mode, b.text, result(d)));

/** Each state: the thread's kind, then what to do once its composer is open. */
const STATES = {
  'idle-empty': { kind: 'idle', run: async () => {} },
  'idle-draft': { kind: 'idle', run: async (p) => type(p.page, TEXT) },
  'busy-queue-steer': { kind: 'busy', run: async (p) => type(p.page, TEXT) },
  'paused-queue': { kind: 'paused', run: async (p) => type(p.page, TEXT) },
  'waits-for-interaction': { kind: 'interaction', run: async (p) => type(p.page, TEXT) },
  'refused-archived': { kind: 'refused', run: async (p) => type(p.page, TEXT) },
  'changed-while-composing': {
    kind: 'idle',
    run: async (p, fx, state) => {
      await type(p.page, TEXT);
      state.kinds[id(1)] = 'busy';
      await p.page.clock.runFor(TIMING.detailIntervalMs);
      await waitFor(async () => (await readComposer(p.page)).changed, { what: 'the change' });
    },
  },
  sending: {
    kind: 'busy',
    run: async (p, fx) => {
      fx.api.hold('send');
      await type(p.page, TEXT);
      await button(p.page, 'Steer').click();
      await outcome(p.page, 'moving');
    },
  },
  'queued-steer-requested': { kind: 'busy', send: answer(DELIVERIES.queuedInteraction), press: 'Steer', shape: 'queued' },
  sent: { kind: 'idle', send: answer(DELIVERIES.sent), press: 'Send', shape: 'sent' },
  failed: { kind: 'idle', send: answer(DELIVERIES.failed), press: 'Send', shape: 'failed' },
  unsupported: { kind: 'busy', send: answer(DELIVERIES.unsupported), press: 'Steer', shape: 'unsupported' },
  'uncertain-core': { kind: 'idle', send: answer(DELIVERIES.uncertainMatched), press: 'Send', shape: 'uncertain' },
  'uncertain-connection-lost': {
    kind: 'idle',
    send: (tid, b) => ({ after: DROP, report: report(tid, b.op_id, b.mode, b.text, result(DELIVERIES.sent)) }),
    press: 'Send',
    shape: 'uncertain',
  },
  'uncertain-checked-no-record': {
    kind: 'idle',
    send: () => DROP,
    press: 'Send',
    shape: 'uncertain',
    after: async (p, fx) => {
      fx.api.setLookup((tid, op) => json(200, { schema: 'openwiz.send-lookup.v1', op_id: op, target: tid, summary: 'no record', result: { kind: 'no_record' } }));
      await button(p.page, 'Check again').click();
      await waitFor(async () => (await readComposer(p.page)).checked, { what: 'the check' });
    },
  },
  'not-sent-busy': { kind: 'idle', send: () => apiError(503, 'busy', 'no send slot within its wait', { 'Retry-After': '5' }), press: 'Send', shape: 'not-sent' },
  'signed-out': { kind: 'idle', send: () => ({ status: 401, headers: { 'Content-Type': 'text/plain' }, body: '' }), press: 'Send', shape: 'not-sent' },
  'possible-duplicate-refused': {
    kind: 'idle',
    send: (tid, b) =>
      json(200, report(tid, b.op_id, b.mode, b.text, { kind: 'possible_duplicate_refused', earlier: [{ op_id: 'web-00000000000000000000000000000001', mode: 'queue', claimed_at: 1_799_913_000_000, in_flight: false, reconciliation: { kind: 'not_read' } }] })),
    press: 'Send',
    shape: 'not-sent',
  },
  'no-longer-listed': {
    kind: 'idle',
    run: async (p, fx) => {
      await type(p.page, TEXT);
      fx.api.setList(() => json(200, inventory([session(2), session(3)])));
      await p.page.clock.runFor(TIMING.listIntervalMs);
      await waitFor(async () => (await readComposer(p.page)).off, { what: 'disabled' });
    },
  },
  'preview-failed': {
    kind: 'idle',
    run: async (p, fx) => {
      fx.api.setPreview(() => apiError(503, 'busy', 'no core run slot within its wait', { 'Retry-After': '5' }));
      await p.page.clock.runFor(TIMING.detailIntervalMs);
      await type(p.page, TEXT);
      await waitFor(async () => (await readComposer(p.page)).actions.every((a) => a.disabled), { what: 'disabled' });
    },
  },
  'storage-full': {
    kind: 'idle',
    run: async (p) => {
      await type(p.page, TEXT);
      await p.page.evaluate(() => {
        Storage.prototype.setItem = () => {
          throw new DOMException('quota', 'QuotaExceededError');
        };
      });
      await button(p.page, 'Send').click();
      await waitFor(async () => (await readComposer(p.page)).off, { what: 'the reason' });
    },
  },
  'retry-refused-still-uncertain': {
    kind: 'idle',
    run: async (p, fx) => {
      let n = 0;
      const earlier = [{ op_id: `web-${'e'.repeat(32)}`, mode: 'queue', claimed_at: 1_799_913_000_000, in_flight: true, reconciliation: { kind: 'not_read' } }];
      fx.api.setSend((tid, b) => (n++ === 0 ? LOST : json(200, report(tid, b.op_id, b.mode, b.text, { kind: 'possible_duplicate_refused', earlier }))));
      await type(p.page, TEXT);
      await button(p.page, 'Send').click();
      await outcome(p.page, 'uncertain');
      await button(p.page, 'Try again').click();
      await waitFor(async () => (await readComposer(p.page)).lines.join(' ').includes('still uncertain'), { what: 'the refused retry' });
    },
  },
  'send-again-failed-earlier-uncertain': {
    kind: 'idle',
    run: async (p, fx) => {
      let n = 0;
      fx.api.setSend((tid, b) =>
        n++ === 0
          ? json(200, report(tid, b.op_id, b.mode, b.text, result(DELIVERIES.uncertain)))
          : json(200, report(tid, b.op_id, b.mode, b.text, result({ kind: 'failed', reason: 'target_archived', text: 'failed: the target is archived, and archived targets are refused' }))),
      );
      await type(p.page, TEXT);
      await button(p.page, 'Send').click();
      await outcome(p.page, 'uncertain');
      await button(p.page, 'Send again').click();
      await waitFor(async () => (await readComposer(p.page)).rows[1]?.outcome === 'failed', { what: 'the failed second attempt' });
    },
  },
  'three-attempts': {
    kind: 'idle',
    run: async (p, fx) => {
      let n = 0;
      fx.api.setSend((tid, b) =>
        n++ < 2 ? json(200, report(tid, b.op_id, b.mode, b.text, result(DELIVERIES.uncertain))) : json(200, report(tid, b.op_id, b.mode, b.text, result(DELIVERIES.sent))),
      );
      await type(p.page, TEXT);
      await button(p.page, 'Send').click();
      await outcome(p.page, 'uncertain');
      for (const i of [2, 3]) {
        await button(p.page, 'Send again').click();
        await waitFor(async () => (await readComposer(p.page)).rows.length === i && (await readComposer(p.page)).outcome !== 'moving', { what: `attempt ${i}` });
      }
    },
  },
  'retry-preview-failed': {
    kind: 'idle',
    run: async (p, fx) => {
      fx.api.setSend(() => LOST);
      await type(p.page, TEXT);
      await button(p.page, 'Send').click();
      await outcome(p.page, 'uncertain');
      fx.api.setPreview(() => apiError(503, 'busy', 'no core run slot within its wait', { 'Retry-After': '5' }));
      await p.page.clock.runFor(TIMING.detailIntervalMs);
      await waitFor(async () => button(p.page, 'Try again').isDisabled(), { what: 'Try again disabled' });
    },
  },
  'file-dropped': {
    kind: 'idle',
    run: async (p) => {
      await type(p.page, TEXT);
      await p.page.evaluate(() => {
        const t = document.querySelector('[data-part=composer] textarea');
        const dt = new DataTransfer();
        dt.items.add(new File(['synthetic'], 'synthetic.txt'));
        t.dispatchEvent(new DragEvent('drop', { dataTransfer: dt, bubbles: true, cancelable: true }));
      });
    },
  },
};

const browser = await launchBrowser();
try {
  for (const [name, s] of Object.entries(STATES)) {
    for (const viewport of [PHONE, DESKTOP]) {
      const { fx, state } = await composerSite({ kinds: { [id(1)]: s.kind } });
      if (s.send) fx.api.setSend(s.send);
      const p = await openThread(browser, fx, 1, { viewport });
      try {
        if (s.run) await s.run(p, fx, state);
        if (s.press) {
          await type(p.page, TEXT);
          await button(p.page, s.press).click();
          await outcome(p.page, s.shape);
        }
        if (s.after) await s.after(p, fx);
        await p.page.locator('[data-part=detail]').evaluate((d) => d.scrollTo(0, d.scrollHeight));
        await p.page.locator('[data-part=composer] textarea').evaluate((t) => t.blur());
        await demo(p.page);
        await p.page.screenshot({ path: `${dir}/${name}-${viewport.width}.png` });
        console.log(`${name}-${viewport.width}.png`);
      } finally {
        await p.close();
        await fx.close();
      }
    }
  }
} finally {
  await browser.close();
}
