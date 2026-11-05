// Renders DEMO-labelled screenshots of the built app on synthetic envelopes,
// every state at 390 and 1440 px, for review: `node test/screens.mjs [out-dir]`.
// Not a proof. Synthetic data only.

import { mkdirSync } from 'node:fs';

import { TIMING } from '../src/timing.ts';
import { DESKTOP, launchBrowser, openPage, PHONE, readDetail, readView, waitFor } from './support/browser.mjs';
import { apiError, html, HANG, json } from './support/fake-api.mjs';
import { fakeSite } from './support/harness.mjs';
import { RESULTS_DIR } from './support/paths.mjs';
import { inventory, manyRows, session, sessionView, statusSessions, T0 } from './support/synthetic.ts';

const dir = process.argv[2] ?? `${RESULTS_DIR}/screens`;
mkdirSync(dir, { recursive: true });
const START = new Date(T0 + 5 * 60_000);

const rows = [
  ...statusSessions(),
  session(6, { group: { kind: 'ungrouped', reason: 'project prj_web_0099 is not in this run\'s project list' }, status: { known: 'idle' } }),
  ...manyRows(14, 10),
];
const gaps = [
  { kind: 'timed_out', subject: null, detail: 'machine list timed out after 15 s' },
  { kind: 'command_failed', subject: 'thr_web_0012', detail: 'thread show exited 1: Error: HTTP 500: Synthetic server error' },
];
// two projects with the same name, and ungrouped rows with the core's reason.
const groupRows = [
  session(21, { group: { kind: 'project', id: 'prj_web_0004', name: { known: 'Synthetic twin project' } }, status: { known: 'active' } }),
  session(22, { group: { kind: 'project', id: 'prj_web_0005', name: { known: 'Synthetic twin project' } } }),
  session(23, { group: { kind: 'ungrouped', reason: 'no project id reported' } }),
  session(24, { group: { kind: 'project', id: 'prj_web_0004', name: { known: 'Synthetic twin project' } }, status: { known: 'error' } }),
  session(25, { group: { kind: 'ungrouped', reason: 'project prj_web_0099 is not in this run\'s project list' }, status: { known: 'syn-other' } }),
  session(26, { group: { kind: 'project', id: 'prj_web_0006', name: { unknown: 'project list did not name it' } } }),
];
const LONG = 'Synthetic session with a very long title that keeps going past the width of any phone and most desktop rows as well, to show how it is cut';
const longRows = [
  session(31, { title: { known: LONG }, status: { known: 'active' } }),
  session(32, {
    title: { known: `Synthetic${'-unbroken'.repeat(16)}` },
    group: { kind: 'ungrouped', reason: `a long synthetic ungrouped reason that runs on ${'and on '.repeat(10)}to the end` },
  }),
  session(33, { title: { unknown: 'missing field title' }, status: { known: `syn-very-long-status-word-${'x'.repeat(40)}` } }),
  session(34, { group: { kind: 'project', id: 'prj_web_0007', name: { known: `Synthetic project with a long name ${'that goes on '.repeat(6)}` } } }),
];

const detailFor = (list) => (id) => {
  const s = list.find((r) => r.id === id) ?? session(1);
  return json(
    200,
    sessionView(
      { ...s, reported_configuration: { model: { known: 'synthetic-model-a' }, effort: { known: 'high' } } },
      { finalOutput: { known: { kind: 'reported', text: 'Synthetic final output.\nTwo lines, as reported.', truncated: false } } },
    ),
  );
};

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

const rowsShown = async (page) => waitFor(async () => (await readView(page)).groups.length > 0, { what: 'rows' });
const open = (id, title) => async (page) => {
  await page.click(`[data-session-id="${id}"]`);
  await waitFor(async () => (await readDetail(page))?.title === title, { what: 'detail' });
};
/** The list's next read fails with `reply` (after one good snapshot). */
const thenFail = (list, reply) => {
  let n = 0;
  return () => (n++ === 0 ? json(200, list) : reply);
};
const nextRead = async (page, want) => {
  await page.clock.runFor(TIMING.listIntervalMs);
  await waitFor(async () => (await readView(page)).connection === want, { what: want });
};

const STATES = [
  { name: 'list', list: () => json(200, inventory(rows)) },
  { name: 'list-greyscale', list: () => json(200, inventory(rows)), grey: true, only: PHONE },
  { name: 'groups', list: () => json(200, inventory(groupRows)), detailRows: groupRows },
  { name: 'detail', list: () => json(200, inventory(rows)), act: open('thr_web_0002', 'Synthetic web session 2') },
  { name: 'partial', list: () => json(200, inventory(rows, { state: 'partial', gaps })) },
  {
    name: 'gaps',
    list: () => json(200, inventory(rows, { state: 'partial', gaps })),
    act: async (page) => page.click('[data-part=coverage]'),
  },
  {
    name: 'stale',
    fails: [inventory(rows), HANG],
    act: async (page) => {
      await page.clock.runFor(TIMING.staleAfterMs + 1_000);
      await waitFor(async () => (await readView(page)).connection === 'stale', { what: 'stale' });
    },
  },
  { name: 'disconnected', fails: [inventory(rows), html(530, 'error code: 1033')], act: (page) => nextRead(page, 'disconnected') },
  { name: 'signed-out', fails: [inventory(rows), html(401, '<html>Synthetic: sign in again</html>')], act: (page) => nextRead(page, 'signed-out') },
  {
    name: 'error',
    fails: [inventory(rows), apiError(502, 'read_failed', 'thread list exited 1: Error: HTTP 500: Synthetic server error')],
    act: (page) => nextRead(page, 'error'),
  },
  { name: 'empty', list: () => json(200, inventory([])), wait: async (page) => waitFor(async () => (await readView(page)).empty, { what: 'empty' }) },
  {
    name: 'empty-partial',
    list: () => json(200, inventory([], { state: 'partial', gaps: gaps.slice(0, 1) })),
    wait: async (page) => waitFor(async () => (await readView(page)).empty, { what: 'empty' }),
  },
  { name: 'long-titles', list: () => json(200, inventory(longRows)), detailRows: longRows },
  { name: 'long-detail', list: () => json(200, inventory(longRows)), detailRows: longRows, act: open('thr_web_0031', LONG) },
  {
    name: 'paging',
    list: (url) => {
      const offset = Number(url.searchParams.get('offset') ?? 0);
      return json(200, inventory(manyRows(100, offset + 1), { offset, matched: offset ? 249 : 250 }));
    },
    act: async (page) => {
      await page.click('[data-part=next]');
      await waitFor(async () => (await readView(page)).range?.startsWith('101'), { what: 'page 2' });
      await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
    },
  },
];

const browser = await launchBrowser();
for (const st of STATES) {
  for (const [label, viewport] of [['390', PHONE], ['1440', DESKTOP]]) {
    if (st.only && st.only !== viewport) continue;
    const fx = await fakeSite();
    fx.api.setList(st.fails ? thenFail(...st.fails) : st.list);
    fx.api.setDetail(detailFor(st.detailRows ?? rows));
    const p = await openPage(browser, fx.site.origin, { viewport, clock: START });
    await (st.wait ?? rowsShown)(p.page);
    if (st.act) await st.act(p.page);
    await demo(p.page);
    if (st.grey) await p.page.evaluate(() => (document.documentElement.style.filter = 'grayscale(1)'));
    await p.page.screenshot({ path: `${dir}/${st.name}-${label}.png` });
    await p.close();
    await fx.close();
  }
}
await browser.close();
console.log(`screenshots in ${dir}`);
