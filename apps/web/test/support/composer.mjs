// Shared setup for the composer's proofs, on the fake
// API's send routes. Synthetic values only.

import { DESKTOP, openPage, readDetail, readView, waitFor } from './browser.mjs';
import { json } from './fake-api.mjs';
import { fakeSite } from './harness.mjs';
import { inventory, preview, session, sessionView, T0 } from './synthetic.ts';

export const START = new Date(T0 + 5 * 60_000);
export const TEXT = 'Synthetic message: reply ok';

/**
 * A site with `count` sessions. `kinds` gives a thread's state for the
 * preview by id (default idle); change it to change what the core says.
 */
export async function composerSite({ count = 3, kinds = {}, finalOutput = null } = {}) {
  const fx = await fakeSite();
  // A thread running a turn (or waiting on an interaction during one) lists as active.
  const status = (n) => ({ known: ['busy', 'interaction'].includes(kinds[id(n)]) ? 'active' : 'idle' });
  const state = { sessions: Array.from({ length: count }, (_, i) => session(i + 1, { status: status(i + 1) })), kinds: { ...kinds } };
  fx.api.setList(() => json(200, inventory(state.sessions)));
  fx.api.setDetail((id) => {
    const s = state.sessions.find((r) => r.id === id) ?? session(Number(id.slice(-4)));
    return json(200, sessionView(s, finalOutput ? { finalOutput } : {}));
  });
  fx.api.setPreview((id, mode) => json(200, preview(id, state.kinds[id] ?? 'idle', mode)));
  return { fx, state };
}

export const id = (n) => `thr_web_${String(n).padStart(4, '0')}`;

/** Opens the page and thread `n`, and waits for its composer's first preview. */
export async function openThread(browser, fx, n, opts = {}) {
  const p = await openPage(browser, fx.site.origin, { clock: START, viewport: DESKTOP, ...opts });
  await waitFor(async () => (await readView(p.page)).groups.length > 0, { what: 'rows' });
  await select(p.page, n);
  return p;
}

export async function select(page, n) {
  // A phone's sheet is modal: close it to reach the list, as the owner does.
  if ((await readDetail(page))?.modal) {
    await page.click('[data-part=detail] .sheet-x');
    await waitFor(async () => (await readDetail(page)) === null, { what: 'the sheet to close' });
  }
  await page.click(`main [data-session-id="${id(n)}"]`);
  await waitFor(async () => (await readDetail(page))?.id === id(n), { what: `the detail of ${id(n)}` });
  await waitFor(async () => (await readComposer(page))?.target === id(n) && (await readComposer(page)).ready, { what: `the composer of ${id(n)}` });
}

/**
 * The composer as the proofs read it, or null. `ready` is the composer's own
 * explicit state (`data-preview`): its preview has answered, usably or not.
 * `outcome`, `word` and `lines` are the newest attempt's; `rows` are all of
 * them, in order.
 */
export function readComposer(page) {
  return page.evaluate(() => {
    const c = document.querySelector('[data-part=detail] [data-part=composer]');
    if (!c) return null;
    const t = c.querySelector('textarea');
    const all = (sel) => [...c.querySelectorAll(sel)];
    const list = c.querySelector('[data-part=outcome]');
    const rows = all('[data-part=attempt]').map((r) => ({
      mode: r.dataset.mode,
      outcome: r.dataset.outcome,
      word: r.querySelector('[data-part=outcome-marker] .word')?.textContent ?? null,
      lines: [...r.querySelectorAll('[data-part=outcome-line]')].map((l) => l.textContent),
      checked: r.querySelector('[data-part=checked]')?.textContent ?? null,
    }));
    const newest = rows.at(-1) ?? null;
    const acts = all('[data-part=actions] [data-action]').map((a) => {
      const b = a.querySelector('button');
      return { key: a.dataset.action, label: b?.textContent ?? null, mode: b?.dataset.mode ?? null, disabled: b ? b.disabled || b.getAttribute('aria-disabled') === 'true' : null, effect: a.querySelector('.effect')?.textContent ?? null };
    });
    return {
      target: c.dataset.target,
      preview: c.dataset.preview,
      draft: t?.value ?? null,
      readOnly: t?.readOnly ?? null,
      focused: document.activeElement === t,
      selection: t ? [t.selectionStart, t.selectionEnd] : null,
      actions: acts,
      ready: c.dataset.preview === 'ready' || c.dataset.preview === 'failed',
      off: c.querySelector('[data-part=send-off]')?.textContent ?? null,
      age: c.querySelector('[data-part=effect-age]')?.textContent ?? null,
      changed: !!c.querySelector('[data-part=effect-changed]'),
      paused: c.querySelector('[data-part=paused]')?.textContent ?? null,
      live: list?.getAttribute('role') ?? null,
      outcome: list?.dataset.outcome ?? null,
      word: newest?.word ?? null,
      lines: newest?.lines ?? [],
      rows,
      checked: all('[data-part=checked]').map((x) => x.textContent).join(' ') || null,
      unsettled: all('[data-part=unsettled] button').map((b) => b.textContent),
      note: c.querySelector('[data-part=composer-note]')?.textContent ?? null,
    };
  });
}

/** Types into the open composer (replacing its text). */
export async function type(page, text) {
  await page.fill('[data-part=composer] textarea', text);
}

export const button = (page, label) => page.locator('[data-part=composer] button', { hasText: new RegExp(`^${label}$`) });

/** The bodies of the POSTs the page made, parsed. */
export const posts = (fx) => fx.api.log.filter((e) => e.kind === 'send' && e.method === 'POST').map((e) => (e.body === undefined ? null : JSON.parse(e.body)));

/** Waits for the composer's outcome to be `shape`. */
export function outcome(page, shape) {
  return waitFor(async () => {
    const c = await readComposer(page);
    return c?.outcome === shape ? c : null;
  }, { what: `the outcome ${shape}` });
}
