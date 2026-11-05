// Renders DEMO-labelled screenshots of the launcher's states  at 390 and
// 1440 px, on the fake API: `node test/launcher-screens.mjs [out-dir]`. Not a
// proof, and not the maintainer acceptance: his design loop is on the working site.
// Synthetic data only.

import { mkdirSync } from 'node:fs';

import { DESKTOP, launchBrowser, PHONE, waitFor } from './support/browser.mjs';
import { apiError, DROP, json } from './support/fake-api.mjs';
import { chooseAll, launcherSite, openLauncher, pick, PROJECT, PROMPT, readLauncher, ready, settled, typePrompt } from './support/launcher.mjs';
import * as nt from './support/newthread-synthetic.ts';
import { RESULTS_DIR } from './support/paths.mjs';

const dir = process.argv[2] ?? `${RESULTS_DIR}/launcher-screens`;
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
    (document.querySelector('dialog[open]:modal') ?? document.body).append(r);
  });

const press = (page) => page.click('[data-part=launcher] [data-part=start]');

/** Each state: what to set up on the fake, then what to do once the launcher is open. */
const STATES = {
  'empty-nothing-preselected': { run: async () => {} },
  'harness-chosen': { run: async (p) => pick(p.page, 'harness', 'codex') },
  'ready-effects-before-pressing': {
    run: async (p) => {
      await chooseAll(p.page);
      await typePrompt(p.page, PROMPT);
      await ready(p.page);
    },
  },
  'switch-cleared-the-model': {
    run: async (p) => {
      await chooseAll(p.page);
      await pick(p.page, 'harness', 'claude-code');
      await typePrompt(p.page, PROMPT);
    },
  },
  'project-refused-before-pressing': {
    setup: (fx) => fx.api.setCreatePreview((project, b) => json(200, nt.preview(project, b, nt.OUTCOMES.noRemote()))),
    run: async (p) => {
      await chooseAll(p.page);
      await typePrompt(p.page, PROMPT);
      await waitFor(async () => (await readLauncher(p.page)).preview === 'ok', { what: 'the preview' });
    },
  },
  creating: {
    setup: (fx) => fx.api.hold('create'),
    run: async (p) => {
      await chooseAll(p.page);
      await typePrompt(p.page, PROMPT);
      await ready(p.page);
      await press(p.page);
      await settled(p.page, 'moving');
    },
  },
  'created-details': {
    run: async (p) => {
      await chooseAll(p.page);
      await typePrompt(p.page, PROMPT);
      await ready(p.page);
      await press(p.page);
      await settled(p.page, 'created');
    },
  },
  'created-reported-differs': {
    setup: (fx) =>
      fx.api.setCreate((project, b) => json(200, nt.report(project, b, nt.OUTCOMES.created('thr_web_9001'), nt.start('thr_web_9001', 'started', b, undefined, 'syn-codex-model-02')))),
    run: async (p) => {
      await chooseAll(p.page);
      await typePrompt(p.page, PROMPT);
      await ready(p.page);
      await press(p.page);
      await settled(p.page, 'created');
    },
  },
  'refused-not-offered': {
    setup: (fx) => fx.api.setCreate((project, b) => json(200, nt.report(project, b, nt.OUTCOMES.notOffered(['syn-codex-model-02'])))),
    run: async (p) => {
      await chooseAll(p.page);
      await typePrompt(p.page, PROMPT);
      await ready(p.page);
      await press(p.page);
      await settled(p.page, 'not-sent');
    },
  },
  'uncertain-connection-lost': {
    setup: (fx) => fx.api.setCreate((project, b) => ({ after: DROP, report: nt.report(project, b, nt.OUTCOMES.uncertain()) })),
    run: async (p) => {
      await chooseAll(p.page);
      await typePrompt(p.page, PROMPT);
      await ready(p.page);
      await press(p.page);
      await settled(p.page, 'uncertain');
    },
  },
  'uncertain-refresh-status': {
    setup: (fx) => {
      fx.api.setCreate((project, b) => json(200, nt.report(project, b, nt.OUTCOMES.uncertain())));
      fx.api.setStatus((op, known) => json(200, nt.status(op, known?.report ?? null, null, 2)));
    },
    run: async (p) => {
      await chooseAll(p.page);
      await typePrompt(p.page, PROMPT);
      await ready(p.page);
      await press(p.page);
      await settled(p.page, 'uncertain');
      await p.page.click('[data-part=launcher] [data-part=refresh-status]');
      await waitFor(async () => (await readLauncher(p.page)).observations, { what: 'the observations' });
    },
  },
  'not-created-busy': {
    setup: (fx) => fx.api.setCreate(() => apiError(503, 'busy', 'too many requests in progress; retry later', { 'Retry-After': '5' })),
    run: async (p) => {
      await chooseAll(p.page);
      await typePrompt(p.page, PROMPT);
      await ready(p.page);
      await press(p.page);
      await settled(p.page, 'not-sent');
    },
  },
  'no-options': {
    setup: (fx) => fx.api.setOptions(() => json(200, nt.noOptions())),
    run: async () => {},
  },
};

const browser = await launchBrowser();
try {
  for (const [name, viewport] of [
    ['390', PHONE],
    ['1440', DESKTOP],
  ]) {
    for (const [state, s] of Object.entries(STATES)) {
      const { fx } = await launcherSite();
      s.setup?.(fx);
      const p = await openLauncher(browser, fx, { viewport });
      try {
        await s.run(p, fx);
        await demo(p.page);
        await p.page.screenshot({ path: `${dir}/launcher-${state}-${name}.png`, fullPage: false });
        // On a phone the sheet scrolls: a second screenshot of the sheet's end.
        if (viewport === PHONE) {
          await p.page.evaluate(() => document.querySelector('dialog[open]')?.scrollTo(0, 1e6));
          await p.page.screenshot({ path: `${dir}/launcher-${state}-${name}-end.png`, fullPage: false });
        }
      } finally {
        await p.close();
        await fx.close();
      }
    }
  }
  console.log(`launcher screens in ${dir} (project ${PROJECT})`);
} finally {
  await browser.close();
}
