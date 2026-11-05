

import type { ClipboardEvent, DragEvent, KeyboardEvent, Ref } from 'react';

import { FOLDER_REFUSED } from '../attachments.ts';
import type { Uploads } from '../attachments.ts';
import { AttachmentTray } from './AttachmentTray.tsx';

/** The words for a refused file, the same wherever the prompt body is. */
export const PASTE_REFUSED = 'a file can’t be pasted: use Add files, or drop it here. Nothing was attached';
export const LOCKED_DROP = 'attachments are fixed while this attempt may have been sent. Nothing was attached';

/** The prompt body's tray: whose it is, and whether it is fixed. */
export type TrayOf = { uploads: Uploads; target: string; locked: boolean };

export function PromptBody({
  fieldId,
  labelId,
  label,
  value,
  readOnly,
  describedBy,
  fieldRef,
  note,
  notePart,
  tray,
  onChange,
  onSubmit,
  onRefuse,
}: {
  fieldId: string;
  labelId: string;
  label: string;
  value: string;
  readOnly: boolean;
  describedBy: string;
  fieldRef?: Ref<HTMLTextAreaElement>;
  /** A refused file's words, or another note about the prompt; null shows nothing. */
  note: string | null;
  notePart: string;
  tray: TrayOf;
  onChange: (text: string) => void;
  /** The keyboard's press: once per key press, never on a held key's repeat. */
  onSubmit: () => void;
  onRefuse: (words: string) => void;
}) {
  const onFiles = (files: File[], note: string | null = null) => void tray.uploads.add(tray.target, files, files.map((f) => f.name), note);
  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key !== 'Enter' || !(e.metaKey || e.ctrlKey)) return;
    e.preventDefault();
    if (!e.repeat) onSubmit();
  };
  const onDrop = (e: DragEvent<HTMLDivElement>) => {
    if (!e.dataTransfer.types.includes('Files')) return;
    e.preventDefault();
    if (tray.locked) return onRefuse(LOCKED_DROP);
    // A folder is refused with its word; every file beside it is attached.
    const files: File[] = [];
    let folders = 0;
    for (const item of [...e.dataTransfer.items]) {
      if (item.kind !== 'file') continue;
      const entry = item.webkitGetAsEntry?.() ?? null;
      const file = item.getAsFile();
      if (entry?.isDirectory) folders++;
      else if (file) files.push(file);
    }
    const word = folders > 0 ? FOLDER_REFUSED : null;
    if (files.length > 0) onFiles(files, word);
    else tray.uploads.note(tray.target, word);
  };
  const onDragOver = (e: DragEvent<HTMLDivElement>) => {
    if (e.dataTransfer.types.includes('Files')) {
      e.preventDefault();
      e.dataTransfer.dropEffect = tray.locked ? 'none' : 'copy';
    }
  };
  const onPaste = (e: ClipboardEvent<HTMLTextAreaElement>) => {
    // A file path pastes as text; a file alone is refused.
    if (e.clipboardData.files.length > 0 && !e.clipboardData.types.includes('text/plain')) {
      e.preventDefault();
      onRefuse(PASTE_REFUSED);
    }
  };
  return (
    <div className="prompt" data-part="prompt-body" onDrop={onDrop} onDragOver={onDragOver}>
      <label id={labelId} className="sec" htmlFor={fieldId}>
        {label}
      </label>
      <textarea
        ref={fieldRef}
        id={fieldId}
        className="compose"
        data-part="draft"
        rows={3}
        value={value}
        readOnly={readOnly}
        aria-describedby={describedBy}
        spellCheck
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={onKeyDown}
        onPaste={onPaste}
      />
      {note ? (
        <p className="tag dashed" data-part={notePart}>
          {note}
        </p>
      ) : null}
      { }
      <AttachmentTray uploads={tray.uploads} target={tray.target} locked={tray.locked} onAdd={(files) => onFiles(files)} />
    </div>
  );
}
