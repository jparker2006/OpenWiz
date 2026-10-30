// A small PNG reader for the greyscale proofs (V2): 8-bit, non-interlaced,
// RGB or RGBA, which is what Chromium's screenshots are. Node's zlib only.

import { inflateSync } from 'node:zlib';

export function decodePng(buf) {
  if (buf.readUInt32BE(0) !== 0x89504e47) throw new Error('not a PNG');
  let off = 8;
  let width = 0;
  let height = 0;
  let channels = 0;
  const idat = [];
  while (off < buf.length) {
    const len = buf.readUInt32BE(off);
    const type = buf.toString('latin1', off + 4, off + 8);
    const data = buf.subarray(off + 8, off + 8 + len);
    if (type === 'IHDR') {
      width = data.readUInt32BE(0);
      height = data.readUInt32BE(4);
      const depth = data[8];
      const color = data[9];
      if (depth !== 8 || data[12] !== 0) throw new Error('unsupported PNG');
      channels = color === 6 ? 4 : color === 2 ? 3 : 0;
      if (!channels) throw new Error(`unsupported colour type ${color}`);
    } else if (type === 'IDAT') idat.push(data);
    else if (type === 'IEND') break;
    off += 12 + len;
  }
  const raw = inflateSync(Buffer.concat(idat));
  const stride = width * channels;
  const out = new Uint8Array(width * height * 4);
  let prev = new Uint8Array(stride);
  for (let y = 0; y < height; y++) {
    const filter = raw[y * (stride + 1)];
    const line = new Uint8Array(raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1)));
    for (let x = 0; x < stride; x++) {
      const a = x >= channels ? line[x - channels] : 0;
      const b = prev[x];
      const c = x >= channels ? prev[x - channels] : 0;
      let add = 0;
      if (filter === 1) add = a;
      else if (filter === 2) add = b;
      else if (filter === 3) add = (a + b) >> 1;
      else if (filter === 4) {
        const p = a + b - c;
        const pa = Math.abs(p - a);
        const pb = Math.abs(p - b);
        const pc = Math.abs(p - c);
        add = pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
      }
      line[x] = (line[x] + add) & 0xff;
    }
    for (let x = 0; x < width; x++) {
      const i = (y * width + x) * 4;
      out[i] = line[x * channels];
      out[i + 1] = line[x * channels + 1];
      out[i + 2] = line[x * channels + 2];
      out[i + 3] = channels === 4 ? line[x * channels + 3] : 255;
    }
    prev = line;
  }
  return { width, height, data: out };
}

/**
 * The shape of an image without its colours. Each pixel is "ink" when its grey
 * level is more than halfway from the background's (the corner pixel, which
 * the caller keeps outside the shape) to the image's most distant grey. The
 * threshold is relative, so the same shape in two colours gives the same mask.
 */
export function inkMask(png) {
  const grey = (p) => 0.2126 * png.data[p * 4] + 0.7152 * png.data[p * 4 + 1] + 0.0722 * png.data[p * 4 + 2];
  const n = png.width * png.height;
  const bg = grey(0);
  let max = 0;
  for (let p = 0; p < n; p++) max = Math.max(max, Math.abs(grey(p) - bg));
  const mask = new Uint8Array(n);
  for (let p = 0; p < n; p++) mask[p] = max > 0 && Math.abs(grey(p) - bg) > max / 2 ? 1 : 0;
  return { width: png.width, height: png.height, mask };
}

/** How many pixels differ between two masks (Infinity when their sizes differ). */
export function maskDifference(a, b) {
  if (a.width !== b.width || a.height !== b.height) return Infinity;
  let diff = 0;
  for (let i = 0; i < a.mask.length; i++) if (a.mask[i] !== b.mask[i]) diff++;
  return diff;
}
