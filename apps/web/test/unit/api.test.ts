// V6 and V8 at the request level: same-origin GETs to the contract's routes
// only, and every response becomes one honest outcome.

import assert from 'node:assert/strict';
import { test } from 'node:test';

import { inventoryUrl, read, REQUEST_HEADERS, serverDone, sessionUrl } from '../../src/api.ts';
import type { Outcome } from '../../src/api.ts';
import { parseInventory } from '../../src/envelope.ts';
import type { Inventory } from '../../src/envelope.ts';
import { inventory, session } from '../support/synthetic.ts';
import { FakeFetch, json } from './support.ts';
import type { FakeReply } from './support.ts';

test('the routes are the contract\'s, with only offset as a query (V8)', () => {
  assert.equal(inventoryUrl(0), '/api/v1/inventory');
  assert.equal(inventoryUrl(100), '/api/v1/inventory?offset=100');
  assert.equal(inventoryUrl(200), '/api/v1/inventory?offset=200');
  assert.equal(sessionUrl('thr_syn_0001'), '/api/v1/sessions/thr_syn_0001');
  // An id can never add a path segment or a query of its own.
  assert.equal(sessionUrl('a/b?c=1#d'), '/api/v1/sessions/a%2Fb%3Fc%3D1%23d');
  assert.equal(sessionUrl('../health'), '/api/v1/sessions/..%2Fhealth');
});

async function outcome(reply: FakeReply | 'network'): Promise<Outcome<Inventory> | 'stopped'> {
  const f = new FakeFetch();
  const pending = read(f.fetch, '/api/v1/inventory', parseInventory, new AbortController().signal);
  if (reply === 'network') f.fail();
  else f.reply(reply);
  return pending;
}

test('every read is a same-origin GET that is never cached or followed', async () => {
  const f = new FakeFetch();
  const pending = read(f.fetch, '/api/v1/inventory', parseInventory, new AbortController().signal);
  f.reply(json(200, inventory([session(1)])));
  await pending;
  const init = f.inits[0];
  assert.ok(init);
  assert.equal(init.method, 'GET');
  assert.equal(init.credentials, 'same-origin');
  assert.equal(init.redirect, 'manual');
  assert.equal(init.cache, 'no-store');
  assert.deepEqual(init.headers, REQUEST_HEADERS);
  assert.equal(REQUEST_HEADERS['X-Requested-With'], 'XMLHttpRequest');
  assert.equal(f.urls[0], '/api/v1/inventory');
});

test('an envelope is ok; anything else at 200 is an unexpected response, not data', async () => {
  const e = inventory([session(1)]);
  assert.deepEqual(await outcome(json(200, e)), { kind: 'ok', envelope: e });
  assert.deepEqual(await outcome(json(200, { schema: 'openwiz.inventory.v1' })), { kind: 'error', status: 200, error: null });
  assert.deepEqual(await outcome({ status: 200, headers: { 'Content-Type': 'text/html' }, body: '<html>Sign in</html>' }), {
    kind: 'error',
    status: 200,
    error: null,
  });
  assert.deepEqual(await outcome({ status: 200, headers: { 'Content-Type': 'application/json' }, body: '{"schema":' }), {
    kind: 'error',
    status: 200,
    error: null,
  });
});

test('signed out: a redirect to the sign-in page, or a 401, is never a network error (V6)', async () => {
  assert.deepEqual(await outcome({ status: 0, type: 'opaqueredirect' }), { kind: 'signed-out', status: null });
  assert.deepEqual(await outcome({ status: 401, headers: { 'Content-Type': 'text/html' }, body: 'Unauthorized' }), {
    kind: 'signed-out',
    status: 401,
  });
});

test('a 403 page is not "signed out": signing in again would not fix a block', async () => {
  assert.deepEqual(await outcome({ status: 403, headers: { 'Content-Type': 'text/html' }, body: 'Synthetic: access denied' }), {
    kind: 'error',
    status: 403,
    error: null,
  });
});

test('the core\'s and the server\'s own errors keep their kind and words (V6)', async () => {
  for (const [status, kind, message] of [
    [502, 'read_failed', 'thread list exited 1: Error: HTTP 500: Synthetic server error'],
    [404, 'not_found', 'session thr_syn_9999 not found'],
    [500, 'untested_version', 'room-cli 0.44.0 is not the tested version 0.43.1'],
    [400, 'invalid_request', '--limit 201 is above the cap of 200'],
    [504, 'deadline', 'no result within the request deadline'],
  ] as const) {
    assert.deepEqual(await outcome(json(status, { error: { kind, message } })), { kind: 'error', status, error: { kind, message } });
  }
});

test('busy: 503 with Retry-After, with or without a body', async () => {
  const body = { error: { kind: 'busy', message: 'the server is busy' } };
  assert.deepEqual(await outcome(json(503, body, { 'Retry-After': '5' })), {
    kind: 'busy',
    status: 503,
    retryAfterMs: 5000,
    error: body.error,
  });
  assert.deepEqual(await outcome({ status: 503, headers: { 'Retry-After': '5' } }), {
    kind: 'busy',
    status: 503,
    retryAfterMs: 5000,
    error: null,
  });
  assert.deepEqual(await outcome({ status: 503, headers: {} }), { kind: 'busy', status: 503, retryAfterMs: null, error: null });
});

test('a 503 with a page of its own is the network\'s, not the server being busy', async () => {
  assert.deepEqual(await outcome({ status: 503, headers: { 'Content-Type': 'text/html' }, body: '<html>Synthetic: service unavailable</html>' }), {
    kind: 'disconnected',
    cause: 'gateway',
    status: 503,
  });
});

test('disconnected: no answer, or the network\'s own error page (V6)', async () => {
  assert.deepEqual(await outcome('network'), { kind: 'disconnected', cause: 'network', status: null });
  for (const status of [502, 504, 521, 522, 523, 524, 530]) {
    const r = await outcome({ status, headers: { 'Content-Type': 'text/html' }, body: '<html>error code: 1033</html>' });
    assert.deepEqual(r, { kind: 'disconnected', cause: 'gateway', status }, String(status));
  }
});

test('a read the page aborts is a timeout, or nothing at all when the page stopped it', async () => {
  for (const [reason, expected] of [
    ['timeout', { kind: 'disconnected', cause: 'timeout', status: null }],
    ['stop', 'stopped'],
  ] as const) {
    const f = new FakeFetch();
    const controller = new AbortController();
    const pending = read(f.fetch, '/api/v1/inventory', parseInventory, controller.signal);
    controller.abort(reason);
    assert.deepEqual(await pending, expected);
  }
});

test('which answers prove the server is done with a read, so the next one may start', async () => {
  const done = async (reply: FakeReply | 'network') => {
    const o = await outcome(reply);
    assert.notEqual(o, 'stopped');
    return serverDone(o as Outcome<Inventory>);
  };
  const busy = { error: { kind: 'busy', message: 'the server is busy' } };
  // openwiz-server's own answers: its run is over (or never started).
  assert.equal(await done(json(200, inventory([session(1)]))), true);
  assert.equal(await done(json(502, { error: { kind: 'read_failed', message: 'x' } })), true);
  assert.equal(await done(json(404, { error: { kind: 'not_found', message: 'x' } })), true);
  assert.equal(await done(json(503, busy, { 'Retry-After': '5' })), true);
  assert.equal(await done({ status: 503, headers: { 'Retry-After': '5' } }), true);
  // Access at the edge: the request never reached the server.
  assert.equal(await done({ status: 0, type: 'opaqueredirect' }), true);
  assert.equal(await done({ status: 401, headers: { 'Content-Type': 'text/html' }, body: 'x' }), true);
  assert.equal(await done({ status: 403, headers: { 'Content-Type': 'text/html' }, body: 'x' }), true);
  // Not known: the server may still be working on it.
  assert.equal(await done(json(504, { error: { kind: 'deadline', message: 'x' } })), false, 'a run past its deadline keeps going (contract)');
  assert.equal(await done('network'), false);
  assert.equal(await done({ status: 530, headers: { 'Content-Type': 'text/html' }, body: 'error code: 1033' }), false);
  assert.equal(await done({ status: 502, headers: { 'Content-Type': 'text/html' }, body: 'x' }), false);
  assert.equal(await done({ status: 503, headers: { 'Content-Type': 'text/html' }, body: 'x' }), false);
  assert.equal(await done({ status: 200, headers: { 'Content-Type': 'text/html' }, body: 'x' }), false);
});

test('Retry-After: whole seconds only; anything else is ignored', async () => {
  for (const [value, ms] of [
    ['5', 5_000],
    ['999999', 999_999_000],
    ['-5', null],
    ['1.5', null],
    ['Wed, 21 Oct 2026 07:28:00 GMT', null],
    ['', null],
  ] as const) {
    const o = await outcome({ status: 503, headers: { 'Retry-After': value } });
    assert.deepEqual(o, { kind: 'busy', status: 503, retryAfterMs: ms, error: null }, JSON.stringify(value));
  }
});
