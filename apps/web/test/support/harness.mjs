// Shared setup for the proofs that run against the fake API.

import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';

import { fakeApi } from './fake-api.mjs';
import { startSite } from './serve.mjs';

const require = createRequire(import.meta.url);
const AXE = readFileSync(require.resolve('axe-core/axe.min.js'), 'utf8');

export async function fakeSite() {
  const api = fakeApi();
  const site = await startSite(api.handler);
  return { api, site, close: () => site.close() };
}

/**
 * axe-core's violations on the page. Injected through the
 * DevTools protocol, which the page's CSP does not govern, and never shipped.
 */
export async function axe(page, runOnly = ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa']) {
  await page.evaluate(AXE);
  const result = await page.evaluate(
    (tags) => window.axe.run(document, { runOnly: { type: 'tag', values: tags }, resultTypes: ['violations'] }),
    runOnly,
  );
  return result.violations.map((v) => ({ id: v.id, impact: v.impact, nodes: v.nodes.map((n) => n.target.join(' ')).slice(0, 5) }));
}
