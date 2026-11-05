// Shared setup for the launcher's proofs , on the fake API's
// new-thread routes. Synthetic values only.

import { DESKTOP, openPage, readDetail, readView, waitFor } from './browser.mjs';
import { json } from './fake-api.mjs';
import { fakeSite } from './harness.mjs';
import { inventory, session, sessionView, T0 } from './synthetic.ts';

export const START = new Date(T0 + 5 * 60_000);
export const PROJECT = 'prj_web_0001';
export const PROMPT = 'Synthetic prompt: reply ok';

/** A site with `count` sessions in one project (prj_web_0001), plus `extra` sessions. */
export async function launcherSite({ count = 3, extra = [] } = {}) {
  const fx = await fakeSite();
  const state = { sessions: [...Array.from({ length: count }, (_, i) => session(i + 1)), ...extra] };
  fx.api.setList(() => json(200, inventory(state.sessions)));
  fx.api.setDetail((id) => {
    const s = state.sessions.find((r) => r.id === id) ?? session(Number(id.slice(-4)));
    return json(200, sessionView(s));
  });
  return { fx, state };
}

/** Opens the page, waits for its rows, and presses `project`'s New thread. */
export async function openLauncher(browser, fx, opts = {}, project = PROJECT) {
  const p = await openPage(browser, fx.site.origin, { clock: START, viewport: DESKTOP, ...opts });
  await waitFor(async () => (await readView(p.page)).groups.length > 0, { what: 'rows' });
  await pressNewThread(p.page, project);
  return p;
}

export async function pressNewThread(page, project = PROJECT) {
  // A phone's sheet is modal: close it to reach the list, as the owner does.
  if ((await readDetail(page))?.modal) {
    await page.click('[data-part=detail] .sheet-x');
    await waitFor(async () => (await readDetail(page)) === null, { what: 'the sheet to close' });
  }
  await page.click(`main [data-new-thread="${project}"]`);
  await waitFor(async () => (await readLauncher(page))?.options !== 'reading', { what: 'the launcher and its options' });
}

/**
 * The launcher as the proofs read it, or null. `picks` are the three
 * controls in order: their value, shown text and offered values.
 */
export function readLauncher(page) {
  return page.evaluate(() => {
    const l = document.querySelector('[data-part=detail] [data-part=launcher], [data-part=detail] [data-part=created]');
    if (!l) return null;
    const all = (sel) => [...l.querySelectorAll(sel)];
    const text = (sel) => l.querySelector(sel)?.textContent ?? null;
    const t = l.querySelector('textarea');
    const start = l.querySelector('[data-part=start]');
    const out = l.querySelector('[data-part=launch-outcome]');
    return {
      kind: l.dataset.part,
      project: l.dataset.projectId ?? null,
      thread: l.dataset.threadId ?? null,
      preview: l.dataset.preview ?? null,
      options: l.querySelector('[data-part=options-read]') ? 'read' : l.querySelector('[data-part=options-failed]') ? 'failed' : 'reading',
      optionsLine: text('[data-part=options-read]'),
      picks: all('[data-part=pick]').map((p) => {
        const s = p.querySelector('select');
        return {
          field: p.dataset.field,
          value: s.value,
          shown: s.selectedOptions[0]?.textContent ?? null,
          label: p.querySelector('label')?.textContent ?? null,
          offered: [...s.options].filter((o) => o.value !== '' && !o.disabled).map((o) => o.value),
          unavailable: [...s.options].filter((o) => o.disabled).map((o) => o.textContent),
          disabled: s.getAttribute('aria-disabled') === 'true',
        };
      }),
      note: text('[data-part=launch-note]'),
      draft: t?.value ?? null,
      readOnly: t?.readOnly ?? null,
      focused: document.activeElement === t,
      effects: all('[data-part=effects] li').map((li) => li.textContent),
      effectsAge: text('[data-part=effects-age]'),
      start: start ? { label: start.textContent, disabled: start.getAttribute('aria-disabled') === 'true' } : null,
      startEffect: text('[data-part=start-effect]'),
      outcome: out?.dataset.outcome ?? null,
      live: out?.getAttribute('role') ?? null,
      word: out?.querySelector('[data-part=outcome-marker] .word')?.textContent ?? null,
      lines: all('[data-part=outcome-line]').map((x) => x.textContent),
      checked: text('[data-part=checked]'),
      observations: text('[data-part=observations]'),
      unresolved: all('[data-part=unresolved] button').map((b) => b.textContent),
      created: text('[data-part=created-line]'),
      startLine: text('[data-part=start-line]'),
      requested: text('[data-field=requested]'),
      reported: all('[data-part=reported-setting]').map((x) => x.textContent),
      reportedLabel: text('[data-part=reported-label]'),
      buttons: all('button').map((b) => b.textContent),
    };
  });
}

/** Chooses `value` in the `field` control (harness, model or effort). */
export async function pick(page, field, value) {
  await page.selectOption(`[data-part=launcher] [data-part=pick][data-field=${field}] select`, value);
}

/** Chooses all three settings, in order. */
export async function chooseAll(page, harness = 'codex', model = 'syn-codex-model-01', effort = 'high') {
  await pick(page, 'harness', harness);
  await pick(page, 'model', model);
  await pick(page, 'effort', effort);
}

export async function typePrompt(page, text) {
  await page.fill('[data-part=launcher] textarea', text);
}

/** Waits until Start thread is pressable: the settings, a prompt, and the core's ready preview for them. */
export function ready(page) {
  return waitFor(async () => {
    const l = await readLauncher(page);
    return l?.start && !l.start.disabled && l.effects.length > 0 ? l : null;
  }, { what: 'Start thread to be ready' });
}

/** Waits for the launcher's outcome shape, or the created details. */
export function settled(page, shape) {
  return waitFor(async () => {
    const l = await readLauncher(page);
    if (shape === 'created') return l?.kind === 'created' ? l : null;
    return l?.outcome === shape ? l : null;
  }, { what: `the outcome ${shape}` });
}

/** The bodies of the creates the page made, parsed. */
export const createPosts = (fx) => fx.api.log.filter((e) => e.kind === 'create' && e.method === 'POST').map((e) => (e.body === undefined ? null : JSON.parse(e.body)));
