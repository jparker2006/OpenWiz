// Renders DEMO-labelled screenshots of the built app on synthetic envelopes,
// for review: `node test/screens.mjs [out-dir]`. Not a proof. Synthetic data only.

import { mkdirSync } from 'node:fs';

import { DESKTOP, launchBrowser, openPage, PHONE, readDetail, readView, waitFor } from './support/browser.mjs';
import { html, json } from './support/fake-api.mjs';
import { fakeSite } from './support/harness.mjs';
import { RESULTS_DIR } from './support/paths.mjs';
import { inventory, manyRows, session, sessionView, statusSessions } from './support/synthetic.ts';

const dir = process.argv[2] ?? `${RESULTS_DIR}/screens`;
mkdirSync(dir, { recursive: true });

const rows = [
  ...statusSessions(),
  session(6, { group: { kind: 'ungrouped', reason: 'project prj_web_0099 is not in this run\'s project list' }, status: { known: 'idle' } }),
  ...manyRows(14, 10),
];
const gaps = [{ kind: 'timed_out', subject: null, detail: 'machine list timed out after 15 s' }];

/** A DEMO ribbon, added through the DevTools protocol (outside the page's CSP), so no screenshot passes for real data. */
const demo = (page) =>
  page.evaluate(() => {
    const r = document.createElement('div');
    r.textContent = 'DEMO · SYNTHETIC DATA · NOTHING IS LIVE';
    Object.assign(r.style, {
      position: 'fixed', left: '0', right: '0', bottom: '0', zIndex: '100', padding: '4px 16px', textAlign: 'center',
      background: '#552583', color: '#e8eaed', font: '600 10px/16px GeistMono, monospace', letterSpacing: '.08em',
    });
    document.body.append(r);
  });

const browser = await launchBrowser();
const fx = await fakeSite();
fx.api.setList(() => json(200, inventory(rows, { state: 'partial', gaps, matched: 240, limit: 100 })));
// `show` reads the session's own detail, so its reported configuration is known
// (the list's `ls` rows say "not read: ls without -l"). Distinct synthetic values.
fx.api.setDetail((id) => {
  const s = rows.find((r) => r.id === id) ?? session(1);
  return json(
    200,
    sessionView(
      { ...s, reported_configuration: { model: { known: 'synthetic-model-a' }, effort: { known: 'high' } } },
      { finalOutput: { known: { kind: 'reported', text: 'Synthetic final output.\nTwo lines, as reported.', truncated: false } } },
    ),
  );
});

async function shot(name, viewport, act = async () => {}, grey = false) {
  const p = await openPage(browser, fx.site.origin, { viewport });
  await waitFor(async () => (await readView(p.page)).groups.length > 0, { what: 'rows' });
  await act(p.page);
  await demo(p.page);
  if (grey) await p.page.evaluate(() => (document.documentElement.style.filter = 'grayscale(1)'));
  await p.page.screenshot({ path: `${dir}/${name}.png` });
  await p.close();
}

const open = async (page) => {
  await page.click('[data-session-id="thr_web_0002"]');
  await waitFor(async () => (await readDetail(page))?.title === 'Synthetic web session 2', { what: 'detail' });
};

await shot('phone-list', PHONE, async (page) => page.click('[data-part=coverage]'));
await shot('phone-list-greyscale', PHONE, async () => {}, true);
await shot('phone-sheet', PHONE, open);
await shot('desktop-panel', DESKTOP, open);
const down = await fakeSite();
let n = 0;
down.api.setList(() => (n++ === 0 ? json(200, inventory(rows.slice(0, 6))) : html(530, 'error code: 1033')));
const p = await openPage(browser, down.site.origin, { viewport: PHONE, clock: new Date('2027-01-15T08:05:00Z') });
await waitFor(async () => (await readView(p.page)).groups.length > 0, { what: 'rows' });
await p.page.clock.runFor(10_000);
await waitFor(async () => (await readView(p.page)).connection === 'disconnected', { what: 'disconnected' });
await demo(p.page);
await p.page.screenshot({ path: `${dir}/phone-disconnected.png` });
await p.close();
await down.close();
await fx.close();
await browser.close();
console.log(`screenshots in ${dir}`);
