// Status as a word, a glyph and a shape (V2). The word is the core's, as
// reported. The tier for each word is a design decision, provisional for
// now; any other word is t5.

import type { FactString } from './envelope.ts';
import { unknownText } from './present.ts';

export type Tier = 't1' | 't2' | 't3' | 't4' | 't5';

export type Marker = { tier: Tier; glyph: string; word: string };

/** The decided words (provisional). */
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
