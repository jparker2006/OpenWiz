// The one attachment tray, in the one prompt body's attachments
// region, so the composer and the launcher show the same one (AW8: `npm run
// check` allows <AttachmentTray> only in PromptBody.tsx, and a file input only
// here). Each row: a thumbnail or a glyph, the name as plain text (cut in the
// middle, bidi-isolated), the size, the readiness as a word, a glyph and a
// shape (never colour alone), and Remove; Retry while the browser still has a
// failed file. Adding is the picker here, or a drop on the prompt body.

import { useEffect, useRef, useSyncExternalStore } from 'react';

import { AFTER_RELOAD, METADATA_NOTE, ORDER_NOTE, sizeText, thumbnail } from '../attachments.ts';
import type { Item, ItemState, Tray, Uploads } from '../attachments.ts';

/** The tray of `target`, kept current: the one hook every tray reads through. */
export function useTray(uploads: Uploads, target: string): Tray {
  return useSyncExternalStore(
    (onChange) => uploads.subscribe(onChange),
    () => uploads.get(target),
    () => uploads.get(target),
  );
}

/** Each readiness: its word, glyph and shape (the composer's outcome shapes, rule 23). */
const SHOWN: Record<ItemState, { word: string; glyph: string; shape: string }> = {
  selected: { word: 'on this device', glyph: '[ ]', shape: 'not-sent' },
  uploading: { word: 'uploading to your controller', glyph: '[~]', shape: 'moving' },
  ready: { word: 'ready on your controller', glyph: '[✓]', shape: 'sent' },
  failed: { word: 'failed', glyph: '[x]', shape: 'failed' },
  gone: { word: 'gone', glyph: '[?]', shape: 'uncertain' },
};

/** A long name, cut in the middle so its end (and extension) stays: the cut is CSS's, never the text's. */
function Name({ name }: { name: string }) {
  const cut = Math.max(0, name.length - 10);
  return (
    <bdi className="att-name" data-part="attachment-name" title={name}>
      <span className="att-head">{name.slice(0, cut)}</span>
      <span className="att-tail">{name.slice(cut)}</span>
    </bdi>
  );
}

function Row({ item, uploads, target, locked, url }: { item: Item; uploads: Uploads; target: string; locked: boolean; url: string | null }) {
  const shown = SHOWN[item.state];
  const thumb = thumbnail(item) && url !== null;
  const progress = item.state === 'uploading' && item.size > 0 ? `${Math.min(100, Math.floor((item.sent / item.size) * 100))}%` : '';
  return (
    <li className="att" data-part="attachment" data-state={item.state} data-kind={item.kind ?? 'unknown'} data-thumb={thumb ? 'yes' : 'no'}>
      <span className="att-pic" aria-hidden="true">
        {thumb ? <img className="att-img" src={url} alt="" width={44} height={44} draggable={false} /> : <span className="att-glyph">{item.kind === 'image' ? '[▣]' : '[≡]'}</span>}
      </span>
      <span className="att-text">
        <Name name={item.name} />
        <span className="att-meta">
          <span className="att-size" data-part="attachment-size">
            {sizeText(item.size)}
          </span>{' '}
          <span className={`oc ${shown.shape}`} data-part="attachment-state">
            <span className="glyph" aria-hidden="true">
              {shown.glyph}
            </span>{' '}
            <span className="word">{shown.word}</span>
          </span>
          <span className="att-progress" aria-hidden="true">
            {progress}
          </span>
        </span>
        {item.why ? (
          <span className="said-line" data-part="attachment-why">
            <bdi>{item.why}</bdi>
          </span>
        ) : null}
        {item.restored && item.state === 'ready' ? (
          <span className="meta" data-part="attachment-reload">
            {AFTER_RELOAD}
          </span>
        ) : null}
      </span>
      <span className="att-acts">
        {item.state === 'failed' && uploads.hasFile(item.key) ? (
          <button type="button" className="btn att-btn" data-part="attachment-retry" aria-disabled={locked ? 'true' : undefined} onClick={() => !locked && uploads.retry(target, item.key)}>
            Retry
          </button>
        ) : null}
        <button
          type="button"
          className="btn att-btn att-x"
          data-part="attachment-remove"
          aria-label={`Remove ${item.name}`}
          aria-disabled={locked ? 'true' : undefined}
          onClick={() => !locked && void uploads.remove(target, item.key)}
        >
          <span aria-hidden="true">×</span>
        </button>
      </span>
    </li>
  );
}

export function AttachmentTray({
  uploads,
  target,
  locked,
  onAdd,
}: {
  uploads: Uploads;
  target: string;
  /** While an attempt may have been sent: the attachments are fixed, as the text is. */
  locked: boolean;
  /** The picker's files, the same way as a drop's. */
  onAdd: (files: File[]) => void;
}) {
  const tray = useTray(uploads, target);
  const picker = useRef<HTMLInputElement>(null);

  // After a reload: the ready ones are checked against the server's list. A read; it never uploads or sends.
  useEffect(() => {
    const ctl = new AbortController();
    void uploads.verify(target, ctl.signal);
    return () => ctl.abort('stop');
  }, [uploads, target]);

  const images = tray.items.some((i) => i.kind === 'image' || (i.kind === null && i.previewable));
  const both = tray.items.some((i) => i.kind === 'file') && tray.items.some((i) => i.kind === 'image');
  return (
    <div className="tray" data-part="tray" data-target={target}>
      <div className="act">
        <button type="button" className="btn" data-part="attach" aria-disabled={locked ? 'true' : undefined} onClick={() => !locked && picker.current?.click()}>
          Add files
        </button>
        <p className="effect">{locked ? 'fixed while this attempt may have been sent' : 'each uploads to your controller; nothing is sent until you press'}</p>
        <input
          ref={picker}
          type="file"
          multiple
          hidden
          data-part="attach-input"
          tabIndex={-1}
          onChange={(e) => {
            const files = [...(e.target.files ?? [])];
            e.target.value = '';
            if (!locked && files.length > 0) onAdd(files);
          }}
        />
      </div>
      {tray.items.length > 0 ? (
        <ul className="atts" aria-label="attachments" data-part="attachments">
          {tray.items.map((item) => (
            <Row key={item.key} item={item} uploads={uploads} target={target} locked={locked} url={uploads.url(item.key)} />
          ))}
        </ul>
      ) : null}
      {images ? (
        <p className="tag dotted" data-part="metadata-note">
          {METADATA_NOTE}
        </p>
      ) : null}
      {both ? (
        <p className="meta" data-part="order-note">
          {ORDER_NOTE}
        </p>
      ) : null}
      <p className="meta" role="status" data-part="tray-note">
        {tray.note ?? ''}
      </p>
    </div>
  );
}
