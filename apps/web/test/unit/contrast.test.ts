// V2: WCAG AA contrast of the stylesheet's own tokens: 4.5:1 for text, 3:1
// for the shapes that tell statuses and states apart (N3).

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const css = readFileSync(new URL('../../styles/globals.css', import.meta.url), 'utf8');
const root = css.slice(css.indexOf(':root {'), css.indexOf('}', css.indexOf(':root {')));
const token = (name: string): string => {
  const m = new RegExp(`--${name}: (#[0-9a-f]{6});`).exec(root);
  assert.ok(m?.[1], `token --${name}`);
  return m[1];
};

const channel = (c: number) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const luminance = (hex: string) => {
  const [r, g, b] = [1, 3, 5].map((i) => channel(parseInt(hex.slice(i, i + 2), 16) / 255));
  return 0.2126 * (r ?? 0) + 0.7152 * (g ?? 0) + 0.0722 * (b ?? 0);
};
const ratio = (a: string, b: string) => {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return ((hi ?? 0) + 0.05) / ((lo ?? 0) + 0.05);
};

// The surfaces text and shapes sit on: the canvas, rows on hover, the sheet.
const SURFACES = ['canvas', 's1', 's2'];

test('text: every text colour on every surface it is used on is at least 4.5:1', () => {
  for (const text of ['ink', 'ink-2', 'ink-3']) {
    for (const surface of SURFACES) {
      const r = ratio(token(text), token(surface));
      assert.ok(r >= 4.5, `--${text} on --${surface}: ${r.toFixed(2)}:1`);
    }
  }
  const onGold = ratio(token('on-light'), token('gold'));
  assert.ok(onGold >= 4.5, `t1's text on gold: ${onGold.toFixed(2)}:1`);
});

test('shapes: every colour that draws a status or state shape is at least 3:1', () => {
  for (const shape of ['gold', 'wiz', 'ink', 'ink-2']) {
    for (const surface of SURFACES) {
      const r = ratio(token(shape), token(surface));
      assert.ok(r >= 3, `--${shape} on --${surface}: ${r.toFixed(2)}:1`);
    }
  }
});


test('shapes: every colour a status or state shape rule draws with is at least 3:1', () => {
  const SHAPE_RULE = /\.mk|\.tag|\.shape|\.row\.on::before|\[data-connection/;
  const DRAWN = /^\s*(border[a-z-]*|outline[a-z-]*|box-shadow|background(-color)?)\s*:\s*(.+)$/;
  const used: string[] = [];
  for (const block of css.replace(/\/\*[\s\S]*?\*\//g, '').matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const selector = (block[1] ?? '').trim();
    if (!SHAPE_RULE.test(selector)) continue;
    for (const decl of (block[2] ?? '').split(';')) {
      const m = DRAWN.exec(decl);
      for (const v of m?.[3]?.matchAll(/var\(--([a-z0-9-]+)\)/g) ?? []) used.push(`${selector} → --${v[1]}`);
    }
  }
  assert.ok(used.length >= 10, `found the shape rules (${used.length})`);
  const weak = used.filter((u) => SURFACES.some((surface) => ratio(token(u.split('--').at(-1) ?? ''), token(surface)) < 3));
  assert.deepEqual(weak, []);
});
