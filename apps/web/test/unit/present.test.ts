// V1 at the text level: values appear word for word, as the CLI prints them;
// rows sit under the group the envelope gives them.

import assert from 'node:assert/strict';
import { test } from 'node:test';

import type { Session } from '../../src/envelope.ts';
import { ageText, boundedText, clockText, factText, groupPage, pageRange, unknownText } from '../../src/present.ts';
import { session } from '../support/synthetic.ts';

const p = (id: string, name: string) => ({ kind: 'project' as const, id, name: { known: name } });
const u = (reason: string) => ({ kind: 'ungrouped' as const, reason });

test('one page groups by project id or ungrouped, in first-appearance order, page order kept', () => {
  const rows: Session[] = [
    session(1, { group: p('prj_web_0001', 'One') }),
    session(2, { group: u('project prj_web_0099 is not in this run\'s project list') }),
    session(3, { group: p('prj_web_0002', 'Same name') }),
    session(4, { group: p('prj_web_0001', 'One') }),
    session(5, { group: u('another synthetic reason') }),
    session(6, { group: p('prj_web_0003', 'Same name') }),
    session(7, { group: p('prj_web_0002', 'Same name') }),
  ];
  const groups = groupPage(rows);
  assert.deepEqual(
    groups.map((g) => [g.key, g.sessions.map((s) => s.id)]),
    [
      ['project:prj_web_0001', ['thr_web_0001', 'thr_web_0004']],
      ['ungrouped', ['thr_web_0002', 'thr_web_0005']],
      ['project:prj_web_0002', ['thr_web_0003', 'thr_web_0007']],
      ['project:prj_web_0003', ['thr_web_0006']],
    ],
  );
  // Every row exactly once.
  assert.deepEqual(groups.flatMap((g) => g.sessions).map((s) => s.id).toSorted(), rows.map((s) => s.id).toSorted());
  assert.deepEqual(groupPage([]), []);
});

test('unknowns and facts are word for word', () => {
  assert.equal(unknownText('missing field title'), 'unknown (missing field title)');
  assert.equal(factText({ known: 'Synthetic' }), 'Synthetic');
  assert.equal(factText({ known: '' }), '');
  assert.equal(factText({ unknown: 'not read: ls without -l' }), 'unknown (not read: ls without -l)');
  assert.equal(boundedText('Synthetic cut', true), 'Synthetic cut…');
  assert.equal(boundedText('Synthetic whole', false), 'Synthetic whole');
});

test('the age is the CLI\'s', () => {
  for (const [ms, text] of [
    [0, '0s'],
    [999, '0s'],
    [59_999, '59s'],
    [60_000, '1m'],
    [3_599_999, '59m'],
    [3_600_000, '1h'],
    [86_399_999, '23h'],
    [86_400_000, '1d'],
    [93_075_719_338, '1077d'],
    [-5_000, '-5s'],
    [-61_000, '-1m'],
  ] as const) {
    assert.equal(ageText(ms), text, String(ms));
  }
});

test('the snapshot time is the device\'s local clock, with the date only on another day', () => {
  process.env['TZ'] = 'UTC';
  const now = new Date(Date.UTC(2027, 0, 15, 9, 30, 0));
  assert.equal(clockText(Date.UTC(2027, 0, 15, 8, 0, 5), now), '08:00:05');
  assert.equal(clockText(Date.UTC(2027, 0, 14, 23, 59, 59), now), '2027-01-14 23:59:59');
});

test('the page range comes from the envelope\'s counts', () => {
  const c = (offset: number, limit: number, shown: number, matched: number) => ({
    listed: matched,
    unreadable: 0,
    excluded_hidden: 0,
    excluded_archived: 0,
    hidden_read: false,
    archived_read: false,
    matched,
    offset,
    limit,
    shown,
  });
  assert.deepEqual(pageRange(c(0, 100, 100, 250)), { first: 1, last: 100, matched: 250, previousOffset: null, nextOffset: 100 });
  assert.deepEqual(pageRange(c(100, 100, 100, 250)), { first: 101, last: 200, matched: 250, previousOffset: 0, nextOffset: 200 });
  assert.deepEqual(pageRange(c(200, 100, 50, 250)), { first: 201, last: 250, matched: 250, previousOffset: 100, nextOffset: null });
  assert.deepEqual(pageRange(c(0, 100, 6, 6)), { first: 1, last: 6, matched: 6, previousOffset: null, nextOffset: null });
  assert.deepEqual(pageRange(c(0, 100, 0, 0)), { first: 0, last: 0, matched: 0, previousOffset: null, nextOffset: null });
  // Past the end after rows went away: a way back, nothing further.
  assert.deepEqual(pageRange(c(300, 100, 0, 250)), { first: 0, last: 0, matched: 250, previousOffset: 200, nextOffset: null });
  assert.deepEqual(pageRange(c(0, 0, 0, 250)), { first: 0, last: 0, matched: 250, previousOffset: null, nextOffset: null });
});
