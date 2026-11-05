// Status uses a word, glyph and shape. Known words map to defined tiers;
// unknown words use t5.

import type { FactString } from './envelope.ts';
import { unknownText } from './present.ts';

export type Tier = 't1' | 't2' | 't3' | 't4' | 't5';

export type Marker = { tier: Tier; glyph: string; word: string };

/** The decided words. */
const TIERS = new Map<string, readonly [Tier, string]>([
  ['error', ['t1', '[x]']],
  ['active', ['t3', '[~]']],
  ['idle', ['t4', '[-]']],
]);

export function statusMarker(status: FactString): Marker {
  if (!('known' in status)) return { tier: 't5', glyph: '[?]', word: unknownText(status.unknown) };
  const [tier, glyph] = TIERS.get(status.known) ?? ['t5', '[?]'];
  return { tier, glyph, word: status.known };
}
