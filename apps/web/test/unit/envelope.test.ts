// V1: the web accepts exactly the `openwiz.inventory.v1` envelope, and nothing
// outside it.

import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { test } from 'node:test';

import { parseApiError, parseInventory, parseSessionView } from '../../src/envelope.ts';
import { hostileInventory, hostileSessions, hostileView, inventory, manyRows, session, sessionView } from '../support/synthetic.ts';

const EXAMPLES = new URL('../../../../tests/fixtures/envelopes/', import.meta.url);
const clone = <T>(v: T): T => structuredClone(v);

test('every 0.0.1 example envelope parses as what it is, and only that', () => {
  const files = readdirSync(EXAMPLES).filter((f) => f.endsWith('.json'));
  assert.ok(files.length >= 5, 'the example envelopes are present');
  for (const f of files) {
    const json: unknown = JSON.parse(readFileSync(new URL(f, EXAMPLES), 'utf8'));
    if (f.startsWith('ls-')) {
      assert.deepEqual(parseInventory(json), json, f);
      assert.equal(parseSessionView(json), null, `${f} is not a session view`);
    } else {
      assert.deepEqual(parseSessionView(json), json, f);
      assert.equal(parseInventory(json), null, `${f} is not an inventory`);
    }
  }
});

test('the synthetic web envelopes parse', () => {
  for (const e of [inventory(manyRows(30)), hostileInventory(), inventory([], { countsNull: true, state: 'partial' })]) {
    assert.deepEqual(parseInventory(clone(e)), e);
  }
  for (const s of hostileSessions()) assert.deepEqual(parseSessionView(clone(hostileView(s))), hostileView(s));
  const s = session(1);
  for (const v of [
    sessionView(s, { finalOutput: { known: { kind: 'no_final_output_yet' } } }),
    sessionView(s, { finalOutput: { unknown: 'thread output timed out' }, reportedError: { unknown: 'thread output timed out' } }),
    sessionView(s, { reportedError: { known: { text: 'Synthetic error', truncated: true } } }),
  ]) {
    assert.deepEqual(parseSessionView(clone(v)), v);
  }
});

type Mutation = [string, (e: any) => void];

const listMutations: Mutation[] = [
  ['an extra top-level key', (e) => (e.revision = 3)],
  ['an extra session key', (e) => (e.data.sessions[0].branch = 'syn/x')],
  ['an extra counts key', (e) => (e.coverage.counts.cursor = 'abc')],
  ['a missing session key', (e) => delete e.data.sessions[0].age_ms],
  ['a missing envelope key', (e) => delete e.gaps],
  ['another schema id', (e) => (e.schema = 'openwiz.inventory.v2')],
  ['a coverage state outside the enum', (e) => (e.coverage.state = 'fresh')],
  ['a gap kind outside the enum', (e) => (e.gaps = [{ kind: 'stale', subject: null, detail: 'x' }])],
  ['a gap subject of the wrong type', (e) => (e.gaps = [{ kind: 'malformed', subject: 7, detail: 'x' }])],
  ['an age that is text', (e) => (e.data.sessions[0].age_ms = { known: '60000' })],
  ['an age that is not whole', (e) => (e.data.sessions[0].age_ms = { known: 1.5 })],
  ['a negative count', (e) => (e.coverage.counts.shown = -1)],
  ['an unknown with an empty reason', (e) => (e.data.sessions[0].title = { unknown: '' })],
  ['a fact with both known and unknown', (e) => (e.data.sessions[0].title = { known: 'a', unknown: 'b' })],
  ['a location kind outside the schema', (e) => (e.data.sessions[0].location = { known: { kind: 'remote' } })],
  ['a local location without its machine', (e) => (e.data.sessions[0].location = { known: { kind: 'local' } })],
  ['a grouping kind outside the schema', (e) => (e.data.sessions[0].group = { kind: 'factory', id: 'x', name: { known: 'x' } })],
  ['an ungrouped session without its reason', (e) => (e.data.sessions[0].group = { kind: 'ungrouped' })],
  ['a purpose over 120 characters', (e) => (e.data.sessions[0].recap = { known: { kind: 'purpose_as_reported', text: 'x'.repeat(121), truncated: true } })],
  ['a recap kind outside the schema', (e) => (e.data.sessions[0].recap = { known: { kind: 'summary', text: 'x', truncated: false } })],
  ['a hidden flag that is text', (e) => (e.data.sessions[0].hidden = { known: 'no' })],
  ['a parent that is a number', (e) => (e.data.sessions[0].parent = { known: 5 })],
  ['sessions that are not a list', (e) => (e.data.sessions = {})],
  ['a snapshot time that is text', (e) => (e.freshness.snapshot_at = 'now')],
  ['a source without version_tested', (e) => delete e.source.version_tested],
];

test('an inventory with anything outside the schema is refused', () => {
  for (const [what, mutate] of listMutations) {
    const e: any = clone(inventory([session(1)]));
    mutate(e);
    assert.equal(parseInventory(e), null, what);
  }
  assert.equal(parseInventory(null), null);
  assert.equal(parseInventory([]), null);
  assert.equal(parseInventory('openwiz.inventory.v1'), null);
});

test('a session view with anything outside the schema is refused', () => {
  const viewMutations: Mutation[] = [
    ['an extra data key', (e) => (e.data.messages = [])],
    ['a final output without truncated', (e) => (e.data.final_output = { known: { kind: 'reported', text: 'x' } })],
    ['a final output kind outside the schema', (e) => (e.data.final_output = { known: { kind: 'log' } })],
    ['a reported error that is text', (e) => (e.data.reported_error = { known: 'boom' })],
    ['a missing session', (e) => delete e.data.session],
    ['a session with an extra key', (e) => (e.data.session.pr = 12)],
  ];
  for (const [what, mutate] of viewMutations) {
    const e: any = clone(sessionView(session(1)));
    mutate(e);
    assert.equal(parseSessionView(e), null, what);
  }
});

test('the API error body parses only in its exact shape', () => {
  const ok = { error: { kind: 'not_found', message: 'session thr_syn_9999 not found' } };
  assert.deepEqual(parseApiError(clone(ok)), ok);
  for (const bad of [
    null,
    {},
    { error: 'not_found' },
    { error: { kind: 'not_found' } },
    { error: { kind: 1, message: 'x' } },
    { error: { kind: 'x', message: 'y', stack: 'z' } },
    { error: { kind: 'x', message: 'y' }, extra: 1 },
  ]) {
    assert.equal(parseApiError(bad), null, JSON.stringify(bad));
  }
});
