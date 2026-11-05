// The attachment tray's presentation state: one list per target (a
// thread's composer, or a project's launcher), and the one place a file is
// uploaded or a staged copy removed. Using the shared upload and removal guards:
//
// - **Readiness in the server's words.** A file is `selected` (on this device:
// never "uploaded"), `uploading` (to the owner's controller), `ready` (only
// after the server's ready answer) or `failed`, with the core's reason, or
// the connection's ("connection lost", "signed out"). After a reload a ready
// one comes back from the server's list, without a preview; anything else is
// `gone`: the browser no longer has the file. Nothing here claims a file
// still exists in the browser.
// - **Uploads start when a file is added, two at a time** (the server's upload
// slots), and never again on their own: Retry is the owner's, only while
// the browser still has the file, and it never sends anything.
// - **Safe previews.** A thumbnail is the browser's own copy of the chosen file
// (an object URL), drawn only when its first bytes are PNG, JPEG, GIF or WebP
// and the server reported it as an image (so within the core's image limit).
// SVG, HTML and everything else get a glyph. Object URLs are revoked on
// removal, on hand-over and on unload.
// - **Store the minimum:** each item's name, size, state, id, kind and why, in
// this tab's sessionStorage (from src/composer.ts). Never a file's bytes.
//
// It works out no kind, limit, floor, gate or retention: the core decides,
// through the server's answers. The page's gate only mirrors the server's.

import { attachmentsUrl, deleteAttachment, getAnswer, postAttachment } from './api.ts';
import type { Answer, FetchLike, Upload, XhrFactory } from './api.ts';
import type { StorageLike } from './composer.ts';
import { exact, is, isNull, list, oneOf, parseApiError, str } from './envelope.ts';
import type { Check } from './envelope.ts';
import type { Timers } from './poller.ts';

/** The server's upload slots: the page never asks for more at once, so it never gets its own `503`. */
export const UPLOADS_AT_ONCE = 2;

/** The page gives up on an upload's answer after this, beyond the server's 110-second upload deadline. */
export const UPLOAD_TIMEOUT_MS = 120_000;

/** A composer's tray, by thread id, and a launcher's, by project id: never the same list. */
export const threadTarget = (id: string): string => `thread:${id}`;
export const launchTarget = (project: string): string => `launch:${project}`;

export type ItemState = 'selected' | 'uploading' | 'ready' | 'failed' | 'gone';
export type Kind = 'image' | 'file';

/** One attachment in a tray. The name is the owner's: untrusted text, shown as plain text only. */
export type Item = {
  key: string;
  name: string;
  size: number;
  state: ItemState;
  /** Bytes the browser has sent so far, while uploading. */
  sent: number;
  /** The server's attachment id, once ready. */
  id: string | null;
  /** The core's kind, once ready. */
  kind: Kind | null;
  /** Why it failed, or is gone, or can't be removed, in words: the core's where it gave them. */
  why: string | null;
  /** Whether a thumbnail may be drawn: the first bytes are one of the four image types (`previewable`). */
  previewable: boolean;
  /** Whether it came back after a reload: then there is never a preview. */
  restored: boolean;
};

export type Tray = { items: Item[]; note: string | null };

export const EMPTY_TRAY: Tray = { items: [], note: null };

/** Words the tray shows. */
export const FOLDER_REFUSED = 'Folders aren’t supported yet.';
export const METADATA_NOTE = 'sent unchanged, including any location data';
export const ORDER_NOTE = 'the agent gets files first, then images';
export const AFTER_RELOAD = 'preview not available after reload';

/**
 * The safe-preview rule: whether the first bytes of a file are a PNG,
 * JPEG, GIF or WebP, the only types a thumbnail is drawn for. Never the
 * attachment's kind, which is the core's: a thumbnail also needs the server to
 * have reported an image.
 */
export function previewable(head: Uint8Array): boolean {
  const at = (i: number) => head[i] ?? -1;
  const ascii = (from: number, text: string) => [...text].every((c, i) => at(from + i) === c.charCodeAt(0));
  const png = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a].every((b, i) => at(i) === b);
  const jpeg = [0xff, 0xd8, 0xff].every((b, i) => at(i) === b);
  const gif = ascii(0, 'GIF87a') || ascii(0, 'GIF89a');
  const webp = ascii(0, 'RIFF') && ascii(8, 'WEBP');
  return png || jpeg || gif || webp;
}

/** Whether the tray may draw this item's thumbnail now. */
export const thumbnail = (item: Item): boolean => item.state === 'ready' && item.kind === 'image' && item.previewable && !item.restored;

/** The gate the page mirrors (the server's is the real one): null when every attachment is ready. */
export function gate(tray: Tray): string | null {
  const n = tray.items.filter((i) => i.state !== 'ready').length;
  if (n === 0) return null;
  return `${n} ${n === 1 ? 'attachment' : 'attachments'} not ready`;
}

/** The ready ids, in the owner's order: what a send or a create carries. */
export const readyIds = (tray: Tray): string[] => tray.items.flatMap((i) => (i.state === 'ready' && i.id !== null ? [i.id] : []));

/** A size in the units an owner reads. */
export function sizeText(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ['KB', 'MB', 'GB'] as const;
  let v = bytes / 1024;
  let u = 0;
  while (v >= 1024 && u < units.length - 1) {
    v /= 1024;
    u++;
  }
  return `${v < 10 ? v.toFixed(1) : Math.round(v)} ${units[u]}`;
}

// The server's answers (contracts/attachment.v1.schema.json's reports), read exactly enough.

const kindCheck = is('image', 'file');
const record = exact({ id: str, sha256: str, size_bytes: (v) => typeof v === 'number' && Number.isSafeInteger(v) && v >= 0, kind: kindCheck, name: str, display_name: str });
const readiness: Check = (v) => typeof v === 'object' && v !== null && !Array.isArray(v);
const staged = exact({ attachment: record, readiness, ready_at: (v) => typeof v === 'number', age_ms: (v) => typeof v === 'number', expires_at: oneOf(isNull, (v) => typeof v === 'number') });
const addReport = exact({
  schema: is('openwiz.attachment-add.v1'),
  ready: list(exact({ index: (v) => v === 0, path: str, staged })),
  failed: list(exact({ index: (v) => v === 0, path: str, readiness, message: str })),
  not_staged: list(exact({ index: (v) => v === 0, path: str, error: str })),
  expired: list(staged),
});
const listReport = exact({
  schema: is('openwiz.attachment-list.v1'),
  attachments: list(staged),
  broken: list(exact({ id: str, readiness, message: str })),
  expired: list(staged),
});
const removeReport = exact({
  schema: is('openwiz.attachment-remove.v1'),
  ids: list(exact({ index: (v) => typeof v === 'number', id: str, result: is('removed', 'expired', 'refused', 'not_found', 'not_removed'), message: str })),
  expired: list(staged),
});

type Record_ = { id: string; size_bytes: number; kind: Kind; display_name: string };
type AddReport = { ready: { staged: { attachment: Record_ } }[]; failed: { message: string }[] };
type ListReport = { attachments: { attachment: Record_ }[] };
type RemoveReport = { ids: { result: string; message: string }[] };
const isAdd = (v: unknown): v is AddReport => addReport(v);
const isList = (v: unknown): v is ListReport => listReport(v);
const isRemove = (v: unknown): v is RemoveReport => removeReport(v);

/** A failed upload's words, beside its `failed` marker: the connection's, where the core gave none. */
export const CONNECTION_LOST = 'connection lost: the upload stopped before an answer came; Retry uploads it again';
export const UNREADABLE = 'couldn’t read the file in this browser: choose it again';
export const SIGNED_OUT = 'signed out: sign in again, then Retry';

/** What one upload's answer means for its item. */
export type Uploaded =
  | { kind: 'ready'; id: string; size: number; type: Kind }
  | { kind: 'failed'; why: string }
  /** The owner removed it while it uploaded: nothing to show. */
  | { kind: 'stopped' };

/**
 * An upload's answer, as its item shows it. Ready only on the server's ready
 * report; the core's failure in the core's words; Access's 401, or its
 * redirect, is "signed out"; no answer is "connection lost" (a copy may have
 * become ready on the controller: if so, nothing claims it, and it expires).
 */
export function uploadAnswer(a: Answer): Uploaded {
  switch (a.kind) {
    case 'stopped':
      return { kind: 'stopped' };
    case 'redirect':
      return { kind: 'failed', why: SIGNED_OUT };
    case 'network':
    case 'timeout':
      return { kind: 'failed', why: CONNECTION_LOST };
    case 'response': {
      if (a.status === 401) return { kind: 'failed', why: SIGNED_OUT };
      if (a.status === 200 && isAdd(a.json)) {
        const ready = a.json.ready[0];
        if (ready) return { kind: 'ready', id: ready.staged.attachment.id, size: ready.staged.attachment.size_bytes, type: ready.staged.attachment.kind };
        const failed = a.json.failed[0];
        if (failed) return { kind: 'failed', why: failed.message };
      }
      const error = parseApiError(a.json)?.error;
      return { kind: 'failed', why: error ? error.message : `HTTP ${a.status}` };
    }
  }
}

// Storage: the minimum, in this tab's sessionStorage, given by src/composer.ts.

const KEY = 'openwiz.attachments.';
type StoredItem = { key: string; name: string; size: number; state: ItemState; id: string | null; kind: Kind | null; why: string | null };
const storedCheck = list(
  exact({
    key: str,
    name: str,
    size: (v) => typeof v === 'number' && Number.isSafeInteger(v) && v >= 0,
    state: is('selected', 'uploading', 'ready', 'failed', 'gone'),
    id: oneOf(isNull, str),
    kind: oneOf(isNull, kindCheck),
    why: oneOf(isNull, str),
  }),
);
const isStored = (v: unknown): v is StoredItem[] => storedCheck(v);

export type UploadsDeps = {
  fetch: FetchLike;
  xhr: XhrFactory;
  storage: StorageLike | null;
  timers: Timers;
  /** The browser's object URLs, for thumbnails of the owner's own files. */
  urls: { create(file: Blob): string; revoke(url: string): void };
};

/** Every target's tray, and the one place a file is uploaded or a staged copy removed. */
export class Uploads {
  private readonly deps: UploadsDeps;
  private readonly trays = new Map<string, Tray>();
  private readonly listeners = new Set<() => void>();
  /** The owner's files, in memory only: what Retry and a thumbnail need. */
  private readonly files = new Map<string, Blob>();
  private readonly urls = new Map<string, string>();
  private readonly running = new Map<string, Upload>();
  private readonly waiting: { target: string; key: string }[] = [];
  private seq = 0;

  constructor(deps: UploadsDeps) {
    this.deps = deps;
  }

  get(target: string): Tray {
    const known = this.trays.get(target);
    if (known) return known;
    const tray = this.load(target);
    this.trays.set(target, tray);
    return tray;
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  /** Whether the browser still has this item's file: what Retry needs. */
  hasFile(key: string): boolean {
    return this.files.has(key);
  }

  /** A thumbnail's object URL, while the tray may draw one. */
  url(key: string): string | null {
    return this.urls.get(key) ?? null;
  }

  /**
   * The owner's files, added at the end, in their order, shown at once as
   * selected (on this device), so the gate holds before a byte is read.
   * Each one's first bytes are read (for the preview
   * rule) before anything else: once the selection is read, only the files
   * the browser read, and still in the tray, are queued to upload, in the
   * owner's order, two at a time. So no answer can make a file ready while
   * its read is pending, and a file that can't be read is never uploaded.
   * `note`: the tray's word with them (a refused folder beside them), in
   * place of any earlier one. Resolves once every file has been looked at.
   */
  add(target: string, files: readonly Blob[], names: readonly string[], note: string | null = null): Promise<void> {
    const added: Item[] = files.map((file, i) => {
      const key = `a${Date.now().toString(36)}${(++this.seq).toString(36)}`;
      this.files.set(key, file);
      return { key, name: names[i] ?? '', size: file.size, state: 'selected', sent: 0, id: null, kind: null, why: null, previewable: false, restored: false };
    });
    const tray = this.get(target);
    this.put(target, { items: [...tray.items, ...added], note });
    return Promise.all(added.map((item) => this.sniff(target, item.key))).then((read) => {
      for (const [i, item] of added.entries()) {
        if (read[i] && this.files.has(item.key)) this.waiting.push({ target, key: item.key });
      }
      this.pump();
    });
  }

  /**
   * Reads one file's first bytes for the preview rule: whether it may be
   * uploaded. A file removed meanwhile stays removed. A file the browser
   * can't read fails, in words, and is never uploaded: it can't be sent,
   * and has no Retry.
   */
  private async sniff(target: string, key: string): Promise<boolean> {
    const file = this.files.get(key);
    if (!file) return false;
    let head: Uint8Array;
    try {
      head = new Uint8Array(await file.slice(0, 16).arrayBuffer());
    } catch {
      if (!this.files.has(key)) return false;
      this.forget(key);
      this.update(target, key, { state: 'failed', why: UNREADABLE });
      return false;
    }
    if (!this.files.has(key)) return false;
    this.update(target, key, { previewable: previewable(head) });
    return true;
  }

  /** Makes a ready image's thumbnail URL, once both its bytes and the server allow one. */
  private thumb(target: string, key: string): void {
    const item = this.get(target).items.find((i) => i.key === key);
    const file = this.files.get(key);
    if (!item || !file || this.urls.has(key) || item.state !== 'ready' || item.kind !== 'image' || !item.previewable) return;
    this.urls.set(key, this.deps.urls.create(file));
    this.notify();
  }

  /** A word for the tray, such as a refused folder. */
  note(target: string, words: string | null): void {
    this.put(target, { ...this.get(target), note: words });
  }

  /** Retry: the same file again, only while the browser still has it. Never sends anything. */
  retry(target: string, key: string): void {
    const item = this.get(target).items.find((i) => i.key === key);
    if (!item || item.state !== 'failed' || !this.files.has(key)) return;
    this.update(target, key, { state: 'selected', sent: 0, why: null });
    this.waiting.push({ target, key });
    this.pump();
  }

  /**
   * Remove: stops an upload, forgets a file, or deletes a ready copy on the
   * controller (refused, and kept, while an operation may use it).
   */
  async remove(target: string, key: string): Promise<void> {
    const item = this.get(target).items.find((i) => i.key === key);
    if (!item) return;
    this.running.get(key)?.abort();
    const w = this.waiting.findIndex((x) => x.key === key);
    if (w >= 0) this.waiting.splice(w, 1);
    if (item.state === 'ready' && item.id !== null) {
      const a = await deleteAttachment(this.deps.fetch, item.id, new AbortController().signal);
      const result = a.kind === 'response' && a.status === 200 && isRemove(a.json) ? a.json.ids[0] : undefined;
      if (result?.result !== 'removed' && result?.result !== 'not_found' && result?.result !== 'expired') {
        const why = result ? result.message : removalFailure(a);
        this.update(target, key, { why: `can’t remove: ${why}` });
        return;
      }
    }
    this.forget(key);
    const tray = this.get(target);
    this.put(target, { ...tray, items: tray.items.filter((i) => i.key !== key) });
  }

  /**
   * After a send was queued or sent, or a thread created: the attachments are
   * CloudRoom's now, and the tray is empty again. It says how many were handed
   * over; never "delivered" or "read".
   */
  handOver(target: string, count: number): void {
    const tray = this.get(target);
    for (const i of tray.items) this.forget(i.key);
    this.put(target, { items: [], note: count > 0 ? `${count} ${count === 1 ? 'attachment' : 'attachments'} handed to CloudRoom` : null });
  }

  /**
   * After a reload: a ready attachment comes back only if the server still
   * lists it (with its name, size and kind, and no preview); anything else is
   * gone. A read: it never uploads, removes or sends.
   */
  async verify(target: string, signal: AbortSignal): Promise<void> {
    const restored = this.get(target).items.filter((i) => i.restored && i.state === 'ready');
    if (restored.length === 0) return;
    const a = await getAnswer(this.deps.fetch, attachmentsUrl(), signal);
    if (a.kind === 'stopped') return;
    const listed = a.kind === 'response' && a.status === 200 && isList(a.json) ? a.json.attachments.map((s) => s.attachment) : null;
    for (const item of restored) {
      if (listed === null) {
        this.update(target, item.key, { why: `couldn’t check it is still on your controller (${removalFailure(a)})` });
        continue;
      }
      const found = listed.find((r) => r.id === item.id);
      if (found) this.update(target, item.key, { name: found.display_name, size: found.size_bytes, kind: found.kind, why: null });
      else this.update(target, item.key, { state: 'gone', why: 'no longer on your controller: removed or expired' });
    }
  }

  /** Revokes every object URL (the page is going away). */
  unload(): void {
    for (const url of this.urls.values()) this.deps.urls.revoke(url);
    this.urls.clear();
  }

  private forget(key: string): void {
    this.files.delete(key);
    const url = this.urls.get(key);
    if (url) this.deps.urls.revoke(url);
    this.urls.delete(key);
  }

  /** Starts waiting uploads while fewer than UPLOADS_AT_ONCE run. */
  private pump(): void {
    while (this.running.size < UPLOADS_AT_ONCE) {
      const next = this.waiting.shift();
      if (!next) return;
      void this.upload(next.target, next.key);
    }
  }

  private async upload(target: string, key: string): Promise<void> {
    const file = this.files.get(key);
    const item = this.get(target).items.find((i) => i.key === key);
    if (!file || !item) return this.pump();
    this.update(target, key, { state: 'uploading', sent: 0, why: null });
    const run = postAttachment(this.deps.xhr, file, item.name, (sent) => this.update(target, key, { sent }), UPLOAD_TIMEOUT_MS);
    this.running.set(key, run);
    const said = uploadAnswer(await run.answer);
    this.running.delete(key);
    if (said.kind === 'ready') {
      this.update(target, key, { state: 'ready', id: said.id, size: said.size, kind: said.type, why: null });
      this.thumb(target, key);
    } else if (said.kind === 'failed') this.update(target, key, { state: 'failed', why: said.why });
    this.pump();
  }

  private update(target: string, key: string, change: Partial<Item>): void {
    const tray = this.get(target);
    if (!tray.items.some((i) => i.key === key)) return;
    this.put(target, { ...tray, items: tray.items.map((i) => (i.key === key ? { ...i, ...change } : i)) });
  }

  private load(target: string): Tray {
    try {
      const raw = this.deps.storage?.getItem(KEY + target) ?? null;
      const json: unknown = raw === null ? null : JSON.parse(raw);
      if (!isStored(json)) return EMPTY_TRAY;
      // After a reload the browser has no file: only a ready copy can come back, from the server.
      const items = json.map(
        (s): Item =>
          s.state === 'ready'
            ? { ...s, sent: 0, previewable: false, restored: true }
            : s.state === 'gone'
              ? { ...s, sent: 0, previewable: false, restored: true }
              : { ...s, state: 'gone', sent: 0, previewable: false, restored: true, id: null, why: 'not uploaded before this page reloaded: choose the file again' },
      );
      return { items, note: null };
    } catch {
      return EMPTY_TRAY;
    }
  }

  private put(target: string, tray: Tray): void {
    this.trays.set(target, tray);
    try {
      const storage = this.deps.storage;
      if (storage) {
        if (tray.items.length === 0) storage.removeItem(KEY + target);
        else storage.setItem(KEY + target, JSON.stringify(tray.items.map(({ key, name, size, state, id, kind, why }): StoredItem => ({ key, name, size, state, id, kind, why }))));
      }
    } catch {
      // Storage full or off: the tray still works in this page; a reload forgets it.
    }
    this.notify();
  }

  private notify(): void {
    for (const listener of [...this.listeners]) listener();
  }
}

function removalFailure(a: Answer): string {
  switch (a.kind) {
    case 'redirect':
      return 'signed out';
    case 'network':
    case 'stopped':
      return 'can’t reach OpenWiz on your Mac';
    case 'timeout':
      return 'no answer in time';
    case 'response': {
      if (a.status === 401) return 'signed out';
      const error = parseApiError(a.json)?.error;
      return error ? `${error.kind}: ${error.message}` : `HTTP ${a.status}`;
    }
  }
}

/**
 * A drop anywhere but a prompt body never opens the file in the browser. A
 * prompt body takes its own drops first.
 */
export function guardDrops(target: { addEventListener(type: string, fn: (e: DragEvent) => void): void; removeEventListener(type: string, fn: (e: DragEvent) => void): void }): () => void {
  const over = (e: DragEvent) => {
    if (!e.defaultPrevented && e.dataTransfer?.types.includes('Files')) {
      e.preventDefault();
      e.dataTransfer.dropEffect = 'none';
    }
  };
  const drop = (e: DragEvent) => {
    if (e.dataTransfer?.types.includes('Files')) e.preventDefault();
  };
  target.addEventListener('dragover', over);
  target.addEventListener('drop', drop);
  return () => {
    target.removeEventListener('dragover', over);
    target.removeEventListener('drop', drop);
  };
}
