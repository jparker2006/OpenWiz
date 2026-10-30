// V2 at the vocabulary level: every status keeps its word, and each tier has
// its own glyph. A word without a decided tier is t5; nothing is guessed.

import assert from 'node:assert/strict';
import { test } from 'node:test';

import { statusMarker } from '../../src/markers.ts';
import { HOSTILE } from '../support/synthetic.ts';

test('the decided words have their tiers', () => {
  assert.deepEqual(statusMarker({ known: 'error' }), { tier: 't1', glyph: '[x]', word: 'error' });
  assert.deepEqual(statusMarker({ known: 'active' }), { tier: 't3', glyph: '[~]', word: 'active' });
  assert.deepEqual(statusMarker({ known: 'idle' }), { tier: 't4', glyph: '[-]', word: 'idle' });
});

test('any other word is t5, word for word, never guessed', () => {
  for (const word of ['syn-other', 'Active', 'IDLE', ' idle', 'running', '', HOSTILE.html, HOSTILE.bidi]) {
    assert.deepEqual(statusMarker({ known: word }), { tier: 't5', glyph: '[?]', word }, JSON.stringify(word));
  }
});

test('an unknown status is t5 with its reason', () => {
  assert.deepEqual(statusMarker({ unknown: 'missing field status' }), {
    tier: 't5',
    glyph: '[?]',
    word: 'unknown (missing field status)',
  });
});

test('every tier in use has its own glyph', () => {
  const markers = [{ known: 'error' }, { known: 'active' }, { known: 'idle' }, { known: 'x' }].map(statusMarker);
  assert.equal(new Set(markers.map((m) => m.glyph)).size, markers.length);
  assert.equal(new Set(markers.map((m) => m.tier)).size, markers.length);
});
