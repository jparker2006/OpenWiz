// Shared setup for the attachment tray's proofs , on the fake
// API's attachment routes. Synthetic files only, made here at test time.

import { deflateSync } from 'node:zlib';

import { waitFor } from './browser.mjs';

const crc32 = (buf) => {
  let c = ~0;
  for (const b of buf) {
    c ^= b;
    for (let k = 0; k < 8; k++) c = (c >>> 1) ^ (0xedb88320 & -(c & 1));
  }
  return ~c >>> 0;
};
const chunk = (type, data) => {
  const head = Buffer.alloc(8);
  head.writeUInt32BE(data.length, 0);
  head.write(type, 4, 'latin1');
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(Buffer.concat([Buffer.from(type, 'latin1'), data])), 0);
  return Buffer.concat([head, data, crc]);
};

/** A valid 4 × 4 PNG of one grey: the browser can decode it. */
export function png() {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(4, 0);
  ihdr.writeUInt32BE(4, 4);
  ihdr[8] = 8;
  ihdr[9] = 0;
  const rows = Buffer.alloc(4 * 5, 0x80);
  for (let r = 0; r < 4; r++) rows[r * 5] = 0;
  return Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk('IHDR', ihdr), chunk('IDAT', deflateSync(rows)), chunk('IEND', Buffer.alloc(0))]);
}

/** A 1 × 1 GIF. */
export const gif = () => Buffer.from('R0lGODlhAQABAIAAAP///wAAACH5BAEAAAAALAAAAAABAAEAAAICRAEAOw==', 'base64');
/** A JPEG signature with filler: never decoded by these proofs. */
export const jpeg = () => Buffer.concat([Buffer.from([0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10]), Buffer.from('JFIF\0'), Buffer.alloc(64, 0x5a)]);
/** A WebP signature with filler. */
export const webp = () => Buffer.concat([Buffer.from('RIFF\0\0\0\0WEBPVP8 ', 'latin1'), Buffer.alloc(64, 0x5a)]);
/** An SVG and an HTML page that would set a flag if they ever ran as markup. */
export const svgWithScript = () => Buffer.from('<svg xmlns="http://www.w3.org/2000/svg" onload="window.__pwned=1"><script>window.__pwned=1</script></svg>\n');
export const htmlWithScript = () => Buffer.from('<!doctype html><title>Synthetic</title><script>window.__pwned=1</script><img src=x onerror="window.__pwned=1">\n');
/** A polyglot: a valid PNG with markup after its end. */
export const polyglot = () => Buffer.concat([png(), htmlWithScript()]);
export const text = (s = 'Synthetic notes\n') => Buffer.from(s);

/** A tray's scope: the composer's or the launcher's prompt body. */
export const COMPOSER = '[data-part=detail] [data-part=composer]';
export const LAUNCHER = '[data-part=detail] [data-part=launcher]';

/** Picks `files` (`{ name, buffer, mimeType? }`) with the tray's own file input. */
export async function addFiles(page, scope, files) {
  await page.setInputFiles(`${scope} [data-part=attach-input]`, files.map((f) => ({ name: f.name, mimeType: f.mimeType ?? 'application/octet-stream', buffer: f.buffer })));
}

/** The tray in `scope`, as the proofs read it, or null. */
export function readTray(page, scope) {
  return page.evaluate((scope) => {
    const root = document.querySelector(`${scope} [data-part=tray]`);
    if (!root) return null;
    const rows = [...root.querySelectorAll('[data-part=attachment]')].map((r) => {
      const img = r.querySelector('img');
      return {
        state: r.dataset.state,
        kind: r.dataset.kind,
        thumb: r.dataset.thumb,
        name: r.querySelector('[data-part=attachment-name]')?.textContent ?? null,
        size: r.querySelector('[data-part=attachment-size]')?.textContent ?? null,
        word: r.querySelector('[data-part=attachment-state] .word')?.textContent ?? null,
        glyph: r.querySelector('[data-part=attachment-state] .glyph')?.textContent ?? null,
        why: r.querySelector('[data-part=attachment-why]')?.textContent ?? null,
        reload: r.querySelector('[data-part=attachment-reload]')?.textContent ?? null,
        retry: !!r.querySelector('[data-part=attachment-retry]'),
        remove: r.querySelector('[data-part=attachment-remove]')?.getAttribute('aria-label') ?? null,
        img: img ? img.getAttribute('src') : null,
      };
    });
    return {
      inPromptBody: !!root.closest('[data-part=prompt-body]'),
      rows,
      note: root.querySelector('[data-part=tray-note]')?.textContent || null,
      metadata: root.querySelector('[data-part=metadata-note]')?.textContent ?? null,
      order: root.querySelector('[data-part=order-note]')?.textContent ?? null,
      attach: root.querySelector('[data-part=attach]')?.getAttribute('aria-disabled') === 'true' ? 'disabled' : 'enabled',
    };
  }, scope);
}

/** Waits until every row in `scope` is in `state` (and there are `n` of them). */
export function trayIs(page, scope, n, state) {
  return waitFor(async () => {
    const t = await readTray(page, scope);
    return t && t.rows.length === n && t.rows.every((r) => r.state === state) ? t : null;
  }, { what: `${n} attachments ${state}` });
}

/** The upload bodies the fake received. */
export const uploadsOf = (fx) => fx.api.uploads;
