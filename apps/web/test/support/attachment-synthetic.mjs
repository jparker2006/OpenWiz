// Synthetic attachment reports for the browser proofs, in the shapes of
// contracts/attachment.v1.schema.json: the add report of one upload, the list
// and the remove reports. The kind comes from the first bytes (PNG, JPEG, GIF,
// WebP are images), as the core decides it; the fixtures never decide more.
// Synthetic values only; every hash is made up.

import { createHash } from 'node:crypto';

import { apiError, json } from './fake-api.mjs';

export const T0 = 1_700_000_000_000;
let next = 0;

/** A fresh synthetic attachment id: `att-` and 32 hex digits. */
export const newId = () => `att-${String(++next).padStart(32, '0')}`;

const isImage = (b) =>
  (b[0] === 0x89 && b[1] === 0x50 && b[2] === 0x4e && b[3] === 0x47) ||
  (b[0] === 0xff && b[1] === 0xd8 && b[2] === 0xff) ||
  b.subarray(0, 4).toString('latin1') === 'GIF8' ||
  (b.subarray(0, 4).toString('latin1') === 'RIFF' && b.subarray(8, 12).toString('latin1') === 'WEBP');

const safe = (name) => (name.split(/[/\\]/).pop() || 'attachment').replace(/[^A-Za-z0-9._-]/g, '_').replace(/^[.-]+/, (m) => '_'.repeat(m.length)).slice(0, 179);

/** One staged record, as the store keeps it. */
export function record(id, name, bytes) {
  return { id, sha256: createHash('sha256').update(bytes).digest('hex'), size_bytes: bytes.length, kind: isImage(bytes) ? 'image' : 'file', name: safe(name), display_name: name };
}

const view = (r) => ({ attachment: r, readiness: { state: 'ready' }, ready_at: T0, age_ms: 0, expires_at: T0 + 86_400_000 });

/** The core's ready answer to an upload: it is staged now. */
export function ready(staged, name, bytes) {
  const r = record(newId(), name, bytes);
  staged.set(r.id, r);
  return json(200, { schema: 'openwiz.attachment-add.v1', ready: [{ index: 0, path: name, staged: view(r) }], failed: [], not_staged: [], expired: [] });
}

/** The core's failure of an upload, with its reason and words. Nothing is staged. */
export function failed(name, reason, message) {
  return json(200, { schema: 'openwiz.attachment-add.v1', ready: [], failed: [{ index: 0, path: name, readiness: { state: 'failed', reason }, message }], not_staged: [], expired: [] });
}

export function listReport(staged) {
  return json(200, { schema: 'openwiz.attachment-list.v1', attachments: [...staged.values()].map(view), broken: [], expired: [] });
}

/** A removal: removed, or not found. */
export function remove(staged, id) {
  if (!/^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(id)) return apiError(400, 'invalid_request', 'attachment rm takes attachment ids, as attachment ls shows them');
  const found = staged.delete(id);
  return json(200, {
    schema: 'openwiz.attachment-remove.v1',
    ids: [{ index: 0, id, result: found ? 'removed' : 'not_found', message: found ? 'its copy and its metadata are deleted' : 'no staged attachment has this id' }],
    expired: [],
  });
}

/** A refused removal: its operation may still use it. */
export function refusedRemoval(id) {
  return json(200, { schema: 'openwiz.attachment-remove.v1', ids: [{ index: 0, id, result: 'refused', message: 'its operation is uncertain, and may still use it, so nothing was removed' }], expired: [] });
}

/** The prompt attachments a report carries for `ids`, from the staged records. */
export function promptAttachments(staged, ids) {
  return ids.map((id) => {
    const r = staged.get(id);
    return { id, sha256: r.sha256, size_bytes: r.size_bytes, kind: r.kind, name: r.name };
  });
}
