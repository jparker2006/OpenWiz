// The launcher  through ticket 1's real openwiz-server and its new-thread
// routes, with the repository's fake room-cli over the `normal` scenario plus
// the new-thread responses (the A1 fixtures of room-cli-v127/new-thread, and a
// project list with a synthetic GitHub remote written by this test, never
// committed), and a private new-thread record. The test proxy plays the
// tunnel. A provider call is a `thread spawn` in the fake room-cli's log.
//
// It proves the page reads the real server's options, preview, report and
// status exactly (U5's web half: the page's exact checks accept the real
// shapes, and its words are the core's), that one press is one spawn (U12),
// and that the status read and a reload never spawn again (U11).

import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';

import { DESKTOP, launchBrowser, openPage, readView, waitFor } from '../support/browser.mjs';
import { chooseAll, pressNewThread, PROMPT, readLauncher, ready, settled, typePrompt } from '../support/launcher.mjs';
import { REPO_DIR } from '../support/paths.mjs';
import { startFixtureServer } from '../support/rust.mjs';
import { startSite } from '../support/serve.mjs';

const HOST = 'openwiz.example.test';
const FIXTURES = `${REPO_DIR}/tests/fixtures/room-cli-v127/new-thread`;
const PROJECT = 'prj_syn_0001';

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(() => browser.close());

/** The new-thread responses: synthetic, written here. */
function responses(spawn) {
  const project = (id, name, remote) => ({ id, kind: 'standard', name, gitRemoteUrl: remote, createdAt: 1_699_100_600_000, updatedAt: 1_699_200_600_000, sources: [] });
  return [
    { argv: ['--version'], stdout: 'v127\n' },
    {
      argv: ['project', 'list', '--json', '--include-personal'],
      stdout: JSON.stringify([project(PROJECT, 'Synthetic project one', 'https://github.com/synthetic-owner/synthetic-repo-0001.git'), project('prj_syn_0002', 'Synthetic project two', null)]),
    },
    { argv: ['cloud', 'status', '--json'], stdout_file: `${FIXTURES}/cloud-status.json` },
    spawn,
    { argv: ['thread', 'show', 'thr_syn_0021', '--json'], stdout_file: `${FIXTURES}/thread-show-pending.json` },
    { argv: ['thread', 'list', '--json'], stdout_file: `${FIXTURES}/thread-list.json` },
  ];
}

async function realSite(spawn = { any_spawn: true, stdout_file: `${FIXTURES}/spawn-receipt-created.json`, creates: true }) {
  const server = await startFixtureServer('normal', { allowedHost: HOST, responses: responses(spawn) });
  const site = await startSite({ port: server.port, allowedHost: HOST });
  return {
    server,
    site,
    spawns: () => server.invocations().filter((a) => a[0] === 'thread' && a[1] === 'spawn'),
    close: async () => {
      await site.close();
      await server.stop();
    },
  };
}

async function open(fx) {
  const p = await openPage(browser, fx.site.origin, { viewport: DESKTOP });
  await waitFor(async () => (await readView(p.page)).groups.length > 0, { what: 'rows' });
  await pressNewThread(p.page, PROJECT);
  return p;
}

test('real server: the options, the core’s effects before pressing, one spawn, and the new thread’s details', async () => {
  const fx = await realSite();
  const p = await open(fx);
  try {
    let l = await readLauncher(p.page);
    assert.equal(l.options, 'read', 'the real options parse');
    assert.deepEqual(l.picks[0].offered, ['codex', 'claude-code'], 'only the two harnesses with models (A-2)');
    assert.ok(l.picks[0].unavailable.length >= 1, 'the others, listed with their reason');
    assert.deepEqual(l.picks.map((x) => x.value), ['', '', ''], 'nothing preselected');
    await chooseAll(p.page, 'codex', 'syn-codex-model-01', 'high');
    await typePrompt(p.page, PROMPT);
    l = await ready(p.page);
    assert.equal(l.effects.length, 7, 'the core’s seven effect lines, from the real preview');
    assert.match(l.effects[0], /^creates one Cloud thread in Synthetic project one and starts its first turn$/);
    assert.match(l.effects[2], /\.env/);
    assert.equal(fx.spawns().length, 0, 'the preview spawned nothing');
    await p.page.click('[data-part=launcher] [data-part=start]');
    l = await settled(p.page, 'created');
    assert.equal(l.thread, 'thr_syn_0021');
    assert.match(l.created, /^created: CloudRoom recorded the thread and its first prompt/);
    assert.match(l.startLine, /^start: waiting to start · read /, 'the start, read once by the create');
    assert.equal(l.requested, 'harness codex · model syn-codex-model-01 · effort high');
    assert.equal(l.reportedLabel, 'reported configuration: what CloudRoom reports, never the serving model');
    const spawns = fx.spawns();
    assert.equal(spawns.length, 1, 'one press, one spawn');
    assert.deepEqual(spawns[0].slice(0, 8), ['thread', 'spawn', '--project', PROJECT, '--machine', 'cloud', '--provider', 'codex']);
    assert.equal(spawns[0].at(-1), PROMPT, 'the prompt, byte for byte, last');
    // Refresh status reads; it never spawns.
    await p.page.click('[data-part=created] [data-part=refresh-start]');
    await waitFor(() => fx.server.invocations().filter((a) => a[1] === 'show' && a[2] === 'thr_syn_0021').length >= 2, { what: 'the status read' });
    assert.equal(fx.spawns().length, 1);
  } finally {
    await p.close();
    await fx.close();
  }
});

test('real server: an uncertain create stays uncertain through a status read and a reload; nothing spawns again', async () => {
  const fx = await realSite({ any_spawn: true, creates: true, abort: true });
  const p = await open(fx);
  try {
    await chooseAll(p.page, 'claude-code', 'syn-claude-model-01', 'medium');
    await typePrompt(p.page, PROMPT);
    await ready(p.page);
    await p.page.click('[data-part=launcher] [data-part=start]');
    let l = await settled(p.page, 'uncertain');
    assert.match(l.lines.join(' '), /uncertain: the provider's command ended without an exit code/, 'the core’s words');
    await p.page.click('[data-part=launcher] [data-part=refresh-status]');
    l = await waitFor(async () => {
      const x = await readLauncher(p.page);
      return x.observations ? x : null;
    }, { what: 'the status read' });
    assert.match(l.observations, /observations only/);
    await p.page.reload();
    await waitFor(async () => (await readView(p.page)).groups.length > 0, { what: 'rows' });
    await pressNewThread(p.page, PROJECT);
    l = await readLauncher(p.page);
    assert.equal(l.word, 'uncertain');
    assert.equal(l.draft, PROMPT);
    assert.equal(fx.spawns().length, 1, 'one spawn, ever');
  } finally {
    await p.close();
    await fx.close();
  }
});
