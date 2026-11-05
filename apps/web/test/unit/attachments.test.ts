// The attachment tray's state: AW5's gate (mirrored), AW6's honest
// states, AW7's safe previews, AW9's reloads and lost connections, and the
// tray's one upload path: two at a time, never retried by itself, a retry
// only while the browser has the file, a removal of a ready copy through the
// server, and object URLs revoked. Synthetic values only.

import assert from 'node:assert/strict';
import { test } from 'node:test';

import type { Answer, XhrLike } from '../../src/api.ts';
import {
  CONNECTION_LOST,
  gate,
  launchTarget,
  previewable,
  readyIds,
  SIGNED_OUT,
  thumbnail,
  threadTarget,
  UPLOADS_AT_ONCE,
  uploadAnswer,
  Uploads,
} from '../../src/attachments.ts';
import type { Item, Tray } from '../../src/attachments.ts';
import type { StorageLike } from '../../src/composer.ts';
import { Composers, edit, EMPTY, keyOf, press } from '../../src/composer.ts';
import { bodyOf, EMPTY as LAUNCH_EMPTY, Launchers, press as launchPress } from '../../src/launcher.ts';
import * as nt from '../support/newthread-synthetic.ts';
import { DELIVERIES, lookup, outcome, report } from '../support/synthetic.ts';
import { FakeClock, FakeFetch, json, settle } from './support.ts';

const T = threadTarget('thr_web_0001');
const bytes = (...b: number[]) => new Uint8Array(b);
const PNG = bytes(0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13);
const JPEG = bytes(0xff, 0xd8, 0xff, 0xe0, 0, 16);
const GIF = new TextEncoder().encode('GIF89a\x01\x00');
const WEBP = new TextEncoder().encode('RIFF\x00\x00\x00\x00WEBPVP8 ');
const SVG = new TextEncoder().encode('<svg xmlns="http://www.w3.org/2000/svg"><script>/* synthetic */</script></svg>');
const HTML = new TextEncoder().encode('<!doctype html><script>/* synthetic */</script>');

test('AW7: only PNG, JPEG, GIF and WebP by their first bytes may get a thumbnail', () => {
  for (const [what, head] of [
    ['png', PNG],
    ['jpeg', JPEG],
    ['gif', GIF],
    ['webp', WEBP],
  ] as const) {
    assert.equal(previewable(head), true, what);
  }
  for (const [what, head] of [
    ['svg', SVG],
    ['html', HTML],
    ['empty', bytes()],
    ['riff not webp', new TextEncoder().encode('RIFF\x00\x00\x00\x00WAVEfmt ')],
    ['heic', new TextEncoder().encode('\x00\x00\x00\x18ftypheic')],
    ['a png signature cut short', PNG.subarray(0, 7)],
  ] as const) {
    assert.equal(previewable(head), false, what);
  }
});

const item = (over: Partial<Item>): Item => ({ key: 'k', name: 'synthetic.png', size: 10, state: 'ready', sent: 0, id: 'att-1', kind: 'image', why: null, previewable: true, restored: false, ...over });

test('AW7: a thumbnail needs the browser’s bytes and the server’s image, and never after a reload', () => {
  assert.equal(thumbnail(item({})), true);
  assert.equal(thumbnail(item({ kind: 'file' })), false, 'a PNG the core staged as a file (it never is): no thumbnail');
  assert.equal(thumbnail(item({ previewable: false })), false, 'an SVG or HTML file the server calls an image: still a glyph');
  assert.equal(thumbnail(item({ restored: true })), false, 'after a reload there is no preview');
  for (const state of ['selected', 'uploading', 'failed', 'gone'] as const) assert.equal(thumbnail(item({ state })), false, state);
});

test('AW5: the gate counts every attachment that is not ready; the ready ids keep the owner’s order', () => {
  const tray: Tray = {
    items: [item({ key: 'a', id: 'att-a' }), item({ key: 'b', state: 'uploading', id: null }), item({ key: 'c', id: 'att-c' }), item({ key: 'd', state: 'failed', id: null })],
    note: null,
  };
  assert.equal(gate(tray), '2 attachments not ready');
  assert.equal(gate({ items: [item({ state: 'selected', id: null })], note: null }), '1 attachment not ready');
  assert.equal(gate({ items: [item({ state: 'gone', id: null })], note: null }), '1 attachment not ready');
  assert.equal(gate({ items: [item({})], note: null }), null);
  assert.equal(gate({ items: [], note: null }), null);
  assert.deepEqual(readyIds(tray), ['att-a', 'att-c']);
});

const res = (status: number, value: unknown): Answer => ({ kind: 'response', status, json: value, empty: false });
const addReady = (id: string, kind = 'file') => ({
  schema: 'openwiz.attachment-add.v1',
  ready: [{ index: 0, path: 'synthetic.txt', staged: { attachment: { id, sha256: '0'.repeat(64), size_bytes: 12, kind, name: 'synthetic.txt', display_name: 'synthetic.txt' }, readiness: { state: 'ready' }, ready_at: 1, age_ms: 0, expires_at: 2 } }],
  failed: [],
  not_staged: [],
  expired: [],
});
const addFailed = (message: string) => ({ schema: 'openwiz.attachment-add.v1', ready: [], failed: [{ index: 0, path: 'synthetic.heic', readiness: { state: 'failed', reason: 'unsupported_heif' }, message }], not_staged: [], expired: [] });

test('AW6: an upload is ready only on the server’s ready report; every other answer is its own failure', () => {
  assert.deepEqual(uploadAnswer(res(200, addReady('att-1', 'image'))), { kind: 'ready', id: 'att-1', size: 12, type: 'image' });
  assert.deepEqual(uploadAnswer(res(200, addFailed('HEIC and HEIF images aren’t supported'))), { kind: 'failed', why: 'HEIC and HEIF images aren’t supported' }, 'the core’s words');
  assert.deepEqual(uploadAnswer({ kind: 'network' }), { kind: 'failed', why: CONNECTION_LOST });
  assert.deepEqual(uploadAnswer({ kind: 'timeout' }), { kind: 'failed', why: CONNECTION_LOST });
  assert.deepEqual(uploadAnswer(res(401, undefined)), { kind: 'failed', why: SIGNED_OUT });
  assert.deepEqual(uploadAnswer({ kind: 'redirect' }), { kind: 'failed', why: SIGNED_OUT });
  assert.match(CONNECTION_LOST, /^connection lost: /);
  assert.match(SIGNED_OUT, /^signed out: /);
  assert.deepEqual(uploadAnswer(res(413, { error: { kind: 'body_too_large', message: 'the file is over the upload cap' } })), { kind: 'failed', why: 'the file is over the upload cap' });
  assert.deepEqual(uploadAnswer({ kind: 'stopped' }), { kind: 'stopped' });
  // A 200 that is not an add report is not ready.
  assert.equal(uploadAnswer(res(200, { ready: [] })).kind, 'failed');
  assert.equal(uploadAnswer(res(200, { ...addReady('att-1'), extra: 1 })).kind, 'failed');
});

/** A scripted XMLHttpRequest: each send is held until the test answers it. */
class FakeXhr implements XhrLike {
  static all: FakeXhr[] = [];
  timeout = 0;
  status = 0;
  responseText = '';
  method = '';
  url = '';
  headers: Record<string, string> = {};
  body: Blob | null = null;
  aborted = false;
  type = 'application/json';
  readonly upload: { onprogress: ((e: ProgressEvent) => unknown) | null } = { onprogress: null };
  onload: ((e: ProgressEvent) => unknown) | null = null;
  onerror: ((e: ProgressEvent) => unknown) | null = null;
  ontimeout: ((e: ProgressEvent) => unknown) | null = null;
  onabort: ((e: ProgressEvent) => unknown) | null = null;
  open(method: string, url: string) {
    this.method = method;
    this.url = url;
  }
  setRequestHeader(name: string, value: string) {
    this.headers[name] = value;
  }
  send(body: Blob) {
    this.body = body;
    FakeXhr.all.push(this);
  }
  abort() {
    this.aborted = true;
    this.onabort?.({} as ProgressEvent);
  }
  getResponseHeader(name: string) {
    return name.toLowerCase() === 'content-type' ? this.type : null;
  }
  progress(loaded: number) {
    this.upload.onprogress?.({ loaded } as ProgressEvent);
  }
  answer(status: number, value: unknown) {
    this.status = status;
    this.responseText = JSON.stringify(value);
    this.onload?.({} as ProgressEvent);
  }
  drop() {
    this.onerror?.({} as ProgressEvent);
  }
}

class MemoryStorage implements StorageLike {
  map = new Map<string, string>();
  getItem(k: string) {
    return this.map.get(k) ?? null;
  }
  setItem(k: string, v: string) {
    this.map.set(k, v);
  }
  removeItem(k: string) {
    this.map.delete(k);
  }
}

function setup(storage = new MemoryStorage()) {
  FakeXhr.all = [];
  const fetch = new FakeFetch();
  const made: string[] = [];
  const revoked: string[] = [];
  const uploads = new Uploads({
    fetch: fetch.fetch,
    xhr: () => new FakeXhr(),
    storage,
    timers: new FakeClock(),
    urls: { create: () => `blob:synthetic-${made.push('u')}`, revoke: (u) => revoked.push(u) },
  });
  return { uploads, fetch, made, revoked, storage };
}

const file = (head: Uint8Array, size = 64) => new Blob([new Uint8Array(head), new Uint8Array(Math.max(0, size - head.length))]);

test('AW6, AW9: files start as selected (on this device), upload two at a time, and never claim more than the server says', async () => {
  const { uploads } = setup();
  await uploads.add(T, [file(PNG), file(SVG), file(HTML)], ['a.png', 'b.svg', 'c.html']);
  await settle();
  const states = () => uploads.get(T).items.map((i) => i.state);
  assert.equal(UPLOADS_AT_ONCE, 2);
  assert.deepEqual(states(), ['uploading', 'uploading', 'selected'], 'two at a time; the third waits, on this device');
  assert.equal(FakeXhr.all.length, 2);
  const [first, second] = FakeXhr.all;
  assert.equal(first?.method, 'POST');
  assert.equal(first?.url, '/api/v1/attachments');
  assert.deepEqual(
    { type: first?.headers['Content-Type'], custom: first?.headers['X-OpenWiz-Send'], name: first?.headers['X-OpenWiz-Name'] },
    { type: 'application/octet-stream', custom: '1', name: 'a.png' },
    'a raw body, never multipart',
  );
  first?.progress(32);
  assert.equal(uploads.get(T).items[0]?.sent, 32);
  first?.answer(200, addReady('att-a', 'image'));
  await settle();
  assert.deepEqual(states(), ['ready', 'uploading', 'uploading'], 'a slot freed starts the next');
  second?.drop();
  await settle();
  assert.equal(uploads.get(T).items[1]?.state, 'failed');
  assert.match(uploads.get(T).items[1]?.why ?? '', /connection lost/);
  // Never retried by itself: no new request after a failure.
  await settle();
  assert.equal(FakeXhr.all.length, 3);
  assert.equal(gate(uploads.get(T)), '2 attachments not ready');
});

test('AW7: a thumbnail URL is made only for a previewable file the server reported as an image, and revoked on removal and hand-over', async () => {
  const { uploads, made, revoked, fetch } = setup();
  await uploads.add(T, [file(PNG), file(SVG)], ['a.png', 'b.svg']);
  await settle();
  FakeXhr.all[0]?.answer(200, addReady('att-a', 'image'));
  // The server calls the SVG an image (it never would): still no thumbnail, since its bytes are not an image.
  FakeXhr.all[1]?.answer(200, addReady('att-b', 'image'));
  await settle();
  assert.equal(made.length, 1, 'one object URL, for the PNG only');
  const [png, svg] = uploads.get(T).items;
  assert.ok(png && svg);
  assert.equal(uploads.url(png.key), 'blob:synthetic-1');
  assert.equal(uploads.url(svg.key), null);
  assert.equal(thumbnail(png), true);
  assert.equal(thumbnail(svg), false);
  fetch.auto = () => json(200, { schema: 'openwiz.attachment-remove.v1', ids: [{ index: 0, id: 'att-a', result: 'removed', message: 'its copy and its metadata are deleted' }], expired: [] });
  await uploads.remove(T, png.key);
  assert.deepEqual(revoked, ['blob:synthetic-1'], 'revoked on removal');
  assert.equal(fetch.inits.at(-1)?.method, 'DELETE');
  assert.equal(fetch.urls.at(-1), '/api/v1/attachments/att-a');
  uploads.handOver(T, 1);
  assert.deepEqual(uploads.get(T), { items: [], note: '1 attachment handed to CloudRoom' }, 'never "delivered" or "read"');
});

test('AW9: Retry only while the browser has the file; it uploads again and never sends', async () => {
  const { uploads, fetch } = setup();
  await uploads.add(T, [file(JPEG)], ['a.jpg']);
  await settle();
  FakeXhr.all[0]?.answer(401, { error: { kind: 'unauthorized', message: 'synthetic' } });
  await settle();
  const key = uploads.get(T).items[0]?.key ?? '';
  assert.match(uploads.get(T).items[0]?.why ?? '', /signed out/);
  uploads.retry(T, key);
  await settle();
  assert.equal(FakeXhr.all.length, 2, 'one upload again');
  assert.equal(fetch.urls.length, 0, 'no send, create or any other request');
  FakeXhr.all[1]?.answer(200, addReady('att-r'));
  await settle();
  assert.equal(uploads.get(T).items[0]?.state, 'ready');
});

test('AW9: a removal stops an upload in flight; a refused removal keeps the copy, with the core’s words', async () => {
  const { uploads, fetch } = setup();
  await uploads.add(T, [file(PNG), file(PNG)], ['a.png', 'b.png']);
  await settle();
  const [a, b] = uploads.get(T).items;
  await uploads.remove(T, a?.key ?? '');
  assert.equal(FakeXhr.all[0]?.aborted, true);
  assert.equal(uploads.get(T).items.length, 1);
  FakeXhr.all[1]?.answer(200, addReady('att-b', 'image'));
  await settle();
  fetch.auto = () => json(200, { schema: 'openwiz.attachment-remove.v1', ids: [{ index: 0, id: 'att-b', result: 'refused', message: 'its operation is uncertain, and may still use it, so nothing was removed' }], expired: [] });
  await uploads.remove(T, b?.key ?? '');
  const kept = uploads.get(T).items[0];
  assert.equal(kept?.state, 'ready');
  assert.match(kept?.why ?? '', /^can’t remove: its operation is uncertain/);
});

test('AW9: after a reload, only a ready copy comes back, from the server, without a preview; the rest are gone, in words', async () => {
  const storage = new MemoryStorage();
  const first = setup(storage);
  await first.uploads.add(T, [file(PNG), file(JPEG), new Blob(['x'])], ['a.png', 'b.jpg', 'c.txt']);
  await settle();
  FakeXhr.all[0]?.answer(200, addReady('att-a', 'image'));
  FakeXhr.all[1]?.answer(200, addReady('att-gone', 'image'));
  await settle();
  // The page reloads while c.txt uploads.
  const second = setup(storage);
  const items = second.uploads.get(T).items;
  assert.deepEqual(
    items.map((i) => [i.state, i.restored, i.previewable]),
    [
      ['ready', true, false],
      ['ready', true, false],
      ['gone', true, false],
    ],
  );
  assert.match(items[2]?.why ?? '', /not uploaded before this page reloaded/);
  assert.equal(second.uploads.hasFile(items[2]?.key ?? ''), false, 'no Retry: the browser no longer has it');
  second.fetch.auto = () =>
    json(200, {
      schema: 'openwiz.attachment-list.v1',
      attachments: [{ attachment: { id: 'att-a', sha256: '0'.repeat(64), size_bytes: 12, kind: 'image', name: 'a.png', display_name: 'a.png' }, readiness: { state: 'ready' }, ready_at: 1, age_ms: 0, expires_at: 2 }],
      broken: [],
      expired: [],
    });
  await second.uploads.verify(T, new AbortController().signal);
  const after = second.uploads.get(T).items;
  assert.deepEqual(
    after.map((i) => i.state),
    ['ready', 'gone', 'gone'],
  );
  assert.match(after[1]?.why ?? '', /no longer on your controller/);
  assert.deepEqual(second.fetch.urls, ['/api/v1/attachments'], 'one read of the list, and nothing else');
  assert.equal(second.fetch.inits[0]?.method, 'GET');
  assert.equal(FakeXhr.all.length, 0, 'a reload never uploads');
  assert.equal(gate(second.uploads.get(T)), '2 attachments not ready');
});

test('AW8: a thread’s tray and a launcher’s are separate lists: switching never moves an attachment', async () => {
  const { uploads } = setup();
  await uploads.add(threadTarget('thr_web_0001'), [file(PNG)], ['a.png']);
  await settle();
  assert.equal(uploads.get(threadTarget('thr_web_0002')).items.length, 0);
  assert.equal(uploads.get(launchTarget('prj_web_0001')).items.length, 0);
});

test('AW5: a send carries the ready ids only when there are some; a text-only body is 0.0.3’s', () => {
  const s = edit(EMPTY, 'Synthetic message', () => `web-${'1'.repeat(32)}`);
  const q = { mode: 'queue' as const, resumePausedQueue: false, acceptPossibleDuplicate: false };
  assert.deepEqual(Object.keys(press(s, q)?.body ?? {}).sort(), ['accept_possible_duplicate', 'mode', 'op_id', 'resume_paused_queue', 'text']);
  assert.deepEqual(press(s, q, ['att-a', 'att-b'])?.body.attachments, ['att-a', 'att-b']);
  const l = { ...LAUNCH_EMPTY, draft: 'Synthetic prompt', root: `web-${'2'.repeat(32)}`, choice: { harness: 'codex', model: 'm', effort: 'high' } };
  assert.deepEqual(bodyOf(l)?.attachments, []);
  assert.deepEqual(launchPress(l, ['att-c'])?.body.attachments, ['att-c']);
});

test('AW5: the composer sends nothing while an attachment is not ready, and hands them over once sent', async () => {
  const { uploads } = setup();
  const fetch = new FakeFetch();
  const id = 'thr_web_0001';
  const composers = new Composers({ fetch: fetch.fetch, storage: new MemoryStorage(), timers: new FakeClock(), newOpId: () => `web-${'3'.repeat(32)}`, uploads });
  composers.edit(id, 'Synthetic message');
  await uploads.add(threadTarget(id), [file(PNG)], ['a.png']);
  await settle();
  const q = { mode: 'queue' as const, resumePausedQueue: false, acceptPossibleDuplicate: false };
  assert.equal(composers.attachmentsOff(id), '1 attachment not ready');
  await composers.press(id, q);
  assert.equal(fetch.urls.length, 0, 'no send while uploading');
  FakeXhr.all[0]?.answer(200, addReady('att-s', 'image'));
  await settle();
  assert.equal(composers.attachmentsOff(id), null);
  fetch.auto = () => json(200, sentReport(id, `web-${'3'.repeat(32)}-q1`, 'Synthetic message'));
  await composers.press(id, q);
  await settle();
  assert.equal(fetch.urls.length, 1);
  assert.deepEqual((JSON.parse(String(fetch.inits[0]?.body)) as { attachments: string[] }).attachments, ['att-s']);
  assert.deepEqual(uploads.get(threadTarget(id)), { items: [], note: '1 attachment handed to CloudRoom' });
});

/** A sent report for `id` (the core's shape, synthetic values), with no attachments listed: the tray counts its own. */
function sentReport(id: string, opId: string, text: string) {
  return {
    schema: 'openwiz.messaging.v1',
    op_id: opId,
    target: { id, title: { known: 'Synthetic' } },
    source: null,
    requested_mode: 'queue',
    status: 'accepted',
    summary: 'Queue requested · sent',
    result: { kind: 'outcome', delivery: { kind: 'accepted', joined_running_turn: { known: false }, text: 'sent: the provider accepted the message' }, replayed: false, recovered: false },
    effects: null,
    text_sha256: '0'.repeat(64),
    text_bytes: new TextEncoder().encode(text).length,
    gaps: [],
    claimed_at: 1,
    dispatching_at: 2,
    finished_at: 3,
  };
}



const q = { mode: 'queue' as const, resumePausedQueue: false, acceptPossibleDuplicate: false };
const lostAnswer = () => Promise.reject(new TypeError('synthetic: the connection dropped'));

test('R1: a send settled by Check again hands its attachments over; one still unresolved keeps them; after a reload too', async () => {
  for (const reload of [false, true]) {
    const storage = new MemoryStorage();
    const first = setup(storage);
    const id = 'thr_web_0001';
    const T1 = threadTarget(id);
    const root = `web-${'4'.repeat(32)}`;
    const opId = `${root}-q1`;
    const make = (u: Uploads, f: FakeFetch) => new Composers({ fetch: f.fetch, storage, timers: new FakeClock(), newOpId: () => root, uploads: u });
    let composers = make(first.uploads, first.fetch);
    composers.edit(id, 'Synthetic first');
    await first.uploads.add(T1, [file(PNG)], ['a.png']);
    await settle();
    FakeXhr.all[0]?.answer(200, addReady('att-r1', 'image'));
    await settle();
    // The send's answer never comes.
    first.fetch.auto = null;
    const pressed = composers.press(id, q);
    await settle();
    first.fetch.fail();
    await pressed;
    let uploads = first.uploads;
    let fetch = first.fetch;
    if (reload) {
      const second = setup(storage);
      uploads = second.uploads;
      fetch = second.fetch;
      composers = make(uploads, fetch);
    }
    const key = keyOf(composers.get(id).attempts[0] ?? { mode: 'queue', n: 1 });
    assert.equal(composers.get(id).root, root, `${reload}: unresolved`);
    assert.deepEqual(readyIds(uploads.get(T1)), ['att-r1'], `${reload}: kept while unresolved`);
    // No record yet: still unresolved, still kept.
    fetch.auto = () => json(200, lookup(id, opId, null));
    await composers.check(id, key);
    assert.deepEqual(readyIds(uploads.get(T1)), ['att-r1'], `${reload}: kept after no record`);
    // The record says sent: the draft is done, and so is its tray.
    fetch.auto = () => json(200, lookup(id, opId, report(id, opId, 'queue', 'Synthetic first', outcome(DELIVERIES.sent))));
    await composers.check(id, key);
    assert.equal(composers.get(id).root, null, `${reload}: settled`);
    assert.deepEqual(uploads.get(T1), { items: [], note: '1 attachment handed to CloudRoom' }, `${reload}: handed over`);
    // The next draft carries nothing of it.
    composers.edit(id, 'Synthetic second');
    assert.deepEqual(readyIds(uploads.get(T1)), []);
  }
});

test('R1: a create settled by Refresh status hands its attachments over; after a reload too', async () => {
  for (const reload of [false, true]) {
    const storage = new MemoryStorage();
    const first = setup(storage);
    const P = 'prj_web_0001';
    const T1 = launchTarget(P);
    const root = `web-${'5'.repeat(32)}`;
    const opId = `${root}-c1`;
    const make = (u: Uploads, f: FakeFetch) => new Launchers({ fetch: f.fetch, storage, timers: new FakeClock(), newOpId: () => root, uploads: u });
    let launchers = make(first.uploads, first.fetch);
    const options = nt.options();
    launchers.edit(P, 'Synthetic prompt');
    launchers.choose(P, 'harness', 'codex', options);
    launchers.choose(P, 'model', 'syn-codex-model-01', options);
    launchers.choose(P, 'effort', 'high', options);
    await first.uploads.add(T1, [file(PNG)], ['a.png']);
    await settle();
    FakeXhr.all[0]?.answer(200, addReady('att-c1', 'image'));
    await settle();
    first.fetch.auto = null;
    const pressed = launchers.create(P);
    await settle();
    first.fetch.fail();
    await pressed;
    let uploads = first.uploads;
    let fetch = first.fetch;
    if (reload) {
      const second = setup(storage);
      uploads = second.uploads;
      fetch = second.fetch;
      launchers = make(uploads, fetch);
    }
    assert.notEqual(launchers.get(P).pending, null, `${reload}: unresolved`);
    assert.deepEqual(readyIds(uploads.get(T1)), ['att-c1'], `${reload}: kept while unresolved`);
    const body = { op_id: opId, harness: 'codex', model: 'syn-codex-model-01', effort: 'high', text: 'Synthetic prompt', attachments: ['att-c1'] };
    fetch.auto = () => json(200, nt.status(opId, nt.report(P, body, nt.OUTCOMES.uncertain())));
    await launchers.check(P);
    assert.deepEqual(readyIds(uploads.get(T1)), ['att-c1'], `${reload}: kept while uncertain`);
    fetch.auto = () => json(200, nt.status(opId, nt.report(P, body, nt.OUTCOMES.created('thr_web_9001')), nt.start('thr_web_9001', 'started', body)));
    await launchers.check(P);
    assert.ok(launchers.get(P).created, `${reload}: created`);
    assert.deepEqual(uploads.get(T1), { items: [], note: '1 attachment handed to CloudRoom' }, `${reload}: handed over`);
  }
});

/** A file whose first bytes come when the test says (or never, or as an error). */
class SlowBlob extends Blob {
  private readonly head: () => Promise<ArrayBuffer>;
  constructor(head: () => Promise<ArrayBuffer>) {
    super([new Uint8Array(64)]);
    this.head = head;
  }
  override slice(): Blob {
    const head = this.head;
    return { arrayBuffer: head } as unknown as Blob;
  }
}

test('R2: the selected rows show at once, before any file is read, and Send waits for them', async () => {
  const { uploads } = setup();
  const fetch = new FakeFetch();
  const id = 'thr_web_0001';
  const T1 = threadTarget(id);
  const composers = new Composers({ fetch: fetch.fetch, storage: new MemoryStorage(), timers: new FakeClock(), newOpId: () => `web-${'6'.repeat(32)}`, uploads });
  composers.edit(id, 'Synthetic message');
  let release: (b: ArrayBuffer) => void = () => {};
  const read = new Promise<ArrayBuffer>((r) => {
    release = r;
  });
  const adding = uploads.add(T1, [new SlowBlob(() => read)], ['slow.png']);
  // Before the file is read: the row is there, on this device, and Send is off.
  assert.deepEqual(uploads.get(T1).items.map((i) => [i.name, i.state]), [['slow.png', 'selected']]);
  assert.equal(composers.attachmentsOff(id), '1 attachment not ready');
  await composers.press(id, q);
  assert.equal(fetch.urls.length, 0, 'nothing is sent without the file');

  await settle();
  assert.equal(FakeXhr.all.length, 0, 'no upload before the read');
  assert.equal(uploads.get(T1).items[0]?.state, 'selected');
  release(new Uint8Array(PNG).buffer);
  await adding;
  await settle();
  assert.equal(uploads.get(T1).items[0]?.previewable, true);
  assert.equal(FakeXhr.all.length, 1, 'read, then uploaded');
  FakeXhr.all[0]?.answer(200, addReady('att-slow', 'image'));
  await settle();
  assert.equal(uploads.get(T1).items[0]?.state, 'ready');
  assert.equal(composers.attachmentsOff(id), null);
});

test('R2: a file the browser can’t read fails in words, uploads nothing, and keeps Send off', async () => {
  const { uploads } = setup();
  const id = 'thr_web_0001';
  const T1 = threadTarget(id);
  let fail: (e: Error) => void = () => {};
  const read = new Promise<ArrayBuffer>((_, reject) => {
    fail = reject;
  });
  const adding = uploads.add(T1, [new SlowBlob(() => read)], ['unreadable.png']);
  await settle();
  assert.equal(uploads.get(T1).items[0]?.state, 'selected', 'on this device while it is read');
  assert.equal(FakeXhr.all.length, 0, 'no upload before the read');
  fail(new Error('synthetic read error'));
  await adding;
  await settle();
  const item = uploads.get(T1).items[0];
  assert.equal(item?.state, 'failed');
  assert.match(item?.why ?? '', /^couldn’t read the file/);
  assert.equal(uploads.hasFile(item?.key ?? ''), false, 'no Retry: choose it again');
  assert.equal(FakeXhr.all.length, 0, 'never uploaded');
  assert.equal(gate(uploads.get(T1)), '1 attachment not ready');
});

test('R2: a file removed while it is being read is gone for good: no upload, no thumbnail', async () => {
  const { uploads, made } = setup();
  const T1 = threadTarget('thr_web_0001');
  let release: (b: ArrayBuffer) => void = () => {};
  const read = new Promise<ArrayBuffer>((r) => {
    release = r;
  });
  const adding = uploads.add(T1, [new SlowBlob(() => read)], ['slow.png']);
  const key = uploads.get(T1).items[0]?.key ?? '';
  await uploads.remove(T1, key);
  release(new Uint8Array(PNG).buffer);
  await adding;
  await settle();
  assert.deepEqual(uploads.get(T1).items, []);
  assert.equal(FakeXhr.all.length, 0, 'never uploaded: removed before it could be queued');
  assert.equal(made.length, 0, 'no thumbnail');
});
