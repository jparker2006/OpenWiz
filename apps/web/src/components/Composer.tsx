

import { useEffect, useRef, useState, useSyncExternalStore } from 'react';

import { threadTarget } from '../attachments.ts';
import type { Uploads } from '../attachments.ts';
import { busy as inFlight, isUnsettled, keyOf, locked, refusal } from '../composer.ts';
import type { Attempt, Composers, Press, PreviewSaid, Result, Said, ThreadState } from '../composer.ts';
import type { Mode } from '../messaging.ts';
import { ageText, unknownText } from '../present.ts';
import { useTray } from './AttachmentTray.tsx';
import { PromptBody } from './PromptBody.tsx';

/** The two previews, each with when its answer arrived (the device's monotonic clock): each read has its own age. */
type Previews = { queue: PreviewSaid | null; queueAt: number; steer: PreviewSaid | null; steerAt: number };

/** What the composer's preview is doing: the test hook and the gate agree on it. */
type PreviewState = 'loading' | 'ready' | 'failed';

const requested = (mode: Mode) => (mode === 'queue' ? 'Queue requested' : 'Steer requested');

/** A send button's gate, from its own preview: the core's words, nothing worked out here. */
type Gate = {
  label: string;
  press: Press;
  /** The core's effect line. */
  effect: string | null;
  /** The core's paused-queue line, when a send would resume it. */
  paused: string | null;
  /** The read's age, in milliseconds. */
  ageMs: number | null;
  /** Why this button can't send, in words; null when it can. */
  off: string | null;
};

/**
 * The one gate for every send button: the thread listed, this tab's storage
 * working, and a usable preview for its mode. (No text, or a request out,
 * also disables every button: `SendControl`'s `idle`.)
 */
function gateOf(label: string, mode: Mode, previews: Previews, blocked: string | null): Gate {
  const said = mode === 'queue' ? previews.queue : previews.steer;
  const base: Gate = { label, press: { mode, resumePausedQueue: false, acceptPossibleDuplicate: false }, effect: null, paused: null, ageMs: null, off: null };
  const gated = (g: Gate): Gate => (g.off === null && blocked !== null ? { ...g, off: blocked } : g);
  if (said === null) return gated({ ...base, off: 'reading what a send would do' });
  // No preview, no send: unlike a valid preview whose effect is unknown, which can send.
  if (said.kind === 'failed') return gated({ ...base, off: `what a send would do couldn’t be read (${said.why})` });
  const p = said.preview;
  if (p.status === 'refused') return gated({ ...base, off: p.refusal?.text ?? p.summary });
  const e = p.effects;
  const at = mode === 'queue' ? previews.queueAt : previews.steerAt;
  const ageMs = e ? e.read_age_ms + Math.max(0, performance.now() - at) : null;
  const effect = e === null ? unknownText('not read') : 'known' in e.effect ? e.effect.known.text : unknownText(e.effect.unknown);
  if (p.status === 'ready') return gated({ ...base, effect, ageMs });
  // needs_acknowledgement: this button is the owner's explicit choice to resume the paused queue.
  const paused = e === null ? unknownText('not read') : 'known' in e.paused_queue ? e.paused_queue.known.text : unknownText(e.paused_queue.unknown);
  return gated({ ...base, label: `${label} and resume queue`, press: { ...base.press, resumePausedQueue: true }, effect, paused, ageMs });
}

/**
 * The draft's buttons, from the core's preview: idle ("starts a turn") is
 * Send; a running turn is Queue (the default) and Steer (explicit); anything
 * else is Queue, with the core's words for its effect. A refused target is a
 * disabled Send, with the core's reason.
 */
function draftButtons(previews: Previews, attachments: boolean): { key: string; label: string; mode: Mode }[] {
  const q = previews.queue;
  const kind = q?.kind === 'ok' && q.preview.effects && 'known' in q.preview.effects.effect ? q.preview.effects.effect.known.kind : null;
  if (kind === 'starts_turn' || (q?.kind === 'ok' && q.preview.status === 'refused')) return [{ key: 'send', label: 'Send', mode: 'queue' }];

  if (kind === 'queues_behind_turn' && !attachments)
    return [
      { key: 'queue', label: 'Queue', mode: 'queue' },
      { key: 'steer', label: 'Steer', mode: 'steer' },
    ];
  return [{ key: 'queue', label: 'Queue', mode: 'queue' }];
}

/** Why Steer isn't offered while the draft has attachments. */
export const NO_STEER = 'Steer can’t carry attachments: Queue them, or remove them to Steer';

/**
 * The only way a send button is rendered (`npm run check`). The gate's
 * effect line, or why it can't send, sits beside it, with the read's age.
 */
function SendControl({
  gate,
  idle,
  part,
  action,
  note,
  changed,
  onPress,
}: {
  gate: Gate;
  /** Text to send and no request out. */
  idle: boolean;
  part: string;
  action: string;
  note?: string;
  changed?: boolean;
  onPress: (p: Press) => void;
}) {
  // One activation, one press . A held key never presses again: its
  // auto-repeat is ignored, and after a press this control stays latched until a fresh activation:
  // a new key press or release, a new pointer down, or a click with no key held (assistive
  // technology clicks without keys).
  const latched = useRef(false);
  const keys = useRef(new Set<string>());
  return (
    <div data-send={action}>
      <div className="act" data-action={action}>
        {/* aria-disabled, not disabled: a focused button stays focused while it can't send (Chromium blurs a disabled one). */}
        <button
          type="button"
          className="btn"
          data-part={part}
          data-mode={gate.press.mode}
          aria-disabled={gate.off !== null || !idle ? 'true' : undefined}
          onKeyDown={(e) => {
            if (e.repeat) {
              e.preventDefault();
              return;
            }
            keys.current.add(e.key);
            latched.current = false;
          }}
          onKeyUp={(e) => {
            keys.current.delete(e.key);
            latched.current = false;
          }}
          onPointerDown={() => {
            latched.current = false;
          }}
          onBlur={() => keys.current.clear()}
          onClick={() => {
            if (keys.current.size === 0) latched.current = false;
            if (latched.current || gate.off !== null || !idle) return;
            latched.current = true;
            onPress(gate.press);
          }}
        >
          {gate.label}
        </button>
        <p className="effect" data-part="effect">
          {gate.off ? (
            <span className="why">
              can’t send: <bdi>{gate.off}</bdi>
            </span>
          ) : (
            <bdi>{`${gate.effect ?? ''}${note ? ` · ${note}` : ''}`}</bdi>
          )}
        </p>
      </div>
      {gate.paused ? (
        <p className="effect paused" data-part="paused">
          <bdi>{gate.paused}</bdi>
        </p>
      ) : null}
      {gate.ageMs !== null ? (
        <p className="meta" data-part="effect-age">
          {/* One text node: a changing number never moves the words after it (V11's layout shift). */}
          {`effect read ${ageText(gate.ageMs)} ago`}
          {changed ? (
            <span className="mod" data-part="effect-changed">
              {' · changed'}
            </span>
          ) : null}
        </p>
      ) : null}
    </div>
  );
}

type Shape = 'sent' | 'queued' | 'failed' | 'unsupported' | 'uncertain' | 'not-sent' | 'moving';

const GLYPHS: Record<Shape, string> = {
  sent: '[✓]',
  queued: '[…]',
  failed: '[x]',
  unsupported: '[/]',
  uncertain: '[?]',
  'not-sent': '[-]',
  moving: '[~]',
};

type Shown = { shape: Shape; word: string; lines: string[] };

/** Why a refusal refused, in its own words. */
function refusedWhy(said: Said): string {
  if (said.kind === 'report') return said.report.summary;
  if (said.kind === 'not-sent') return said.error ? `${said.error.kind}: ${said.error.message}` : `HTTP ${said.status}`;
  return 'signed out';
}

/** One answer, shown as its attempt's own outcome, in the core's words where it gave any. */
function saidShown(said: Said, mode: Mode): Shown {
  switch (said.kind) {
    case 'report': {
      const r = said.report;
      const lines = [r.summary];
      if (r.result.kind === 'outcome' && r.result.delivery.kind === 'uncertain') {
        const d = r.result.delivery;
        if (d.reconciliation.kind === 'observed') lines.push(`observed ${d.reconciliation.matching} matching of ${d.reconciliation.total}`);
        lines.push(d.attribution);
      }
      if (r.result.kind === 'possible_duplicate_refused') {
        const n = r.result.earlier.length;
        lines.push(`${n} earlier ${n === 1 ? 'send' : 'sends'} of this text ${n === 1 ? 'is' : 'are'} in flight or uncertain`);
      }
      switch (r.status) {
        case 'queued':
          return { shape: 'queued', word: 'queued', lines };
        case 'accepted':
          return { shape: 'sent', word: 'sent', lines };
        case 'failed': {
          const unsupported = r.result.kind === 'outcome' && r.result.delivery.kind === 'failed' && r.result.delivery.reason === 'unsupported_mode';
          return unsupported ? { shape: 'unsupported', word: 'unsupported', lines } : { shape: 'failed', word: 'failed', lines };
        }
        case 'uncertain':
          return { shape: 'uncertain', word: 'uncertain', lines };
        case 'in_progress':
          return { shape: 'moving', word: 'in progress', lines };
        case 'conflicting_reuse':
        case 'possible_duplicate_refused':
          return { shape: 'not-sent', word: 'not sent', lines };
      }
      break;
    }
    case 'signed-out':
      return { shape: 'not-sent', word: 'signed out', lines: ['this attempt was not sent; sign in again, then send'] };
    case 'not-sent':
      return { shape: 'not-sent', word: 'not sent', lines: [refusedWhy(said)] };
    case 'unknown': {
      const why =
        said.cause === 'network'
          ? 'the connection was lost before an answer'
          : said.cause === 'timeout'
            ? 'no answer within 45 s'
            : said.cause === 'body'
              ? 'the answer was not a send report'
              : said.cause === 'reload'
                ? 'the page reloaded during the send'
                : `HTTP ${said.status ?? ''}${said.error ? `: ${said.error.kind}: ${said.error.message}` : ''}`;
      return { shape: 'uncertain', word: 'uncertain', lines: [`${requested(mode)} · ${why}`, 'it may or may not have been sent; the text stays'] };
    }
  }
  return { shape: 'uncertain', word: 'uncertain', lines: [] };
}

const RESULT_SHOWN: Record<Result, [Shape, string]> = {
  queued: ['queued', 'queued'],
  sent: ['sent', 'sent'],
  failed: ['failed', 'failed'],
  unsupported: ['unsupported', 'unsupported'],
  'not-sent': ['not-sent', 'not sent'],
  'signed-out': ['not-sent', 'signed out'],
  duplicate: ['not-sent', 'not sent'],
  conflict: ['not-sent', 'not sent'],
};

/** One attempt: in flight, what its own last answer said, or, after a reload, its stored state. */
function attemptShown(a: Attempt, said: Said | undefined): Shown {
  if (a.state === 'sending') return { shape: 'moving', word: 'sending', lines: [`${requested(a.mode)} · waiting for the answer`] };
  if (a.state === 'checking') return { shape: 'moving', word: 'checking', lines: ['looking up this send’s record; nothing is sent'] };
  if (said === undefined) {
    // After a reload, only the stored attempt is left: the reports stayed in memory.
    if (a.result !== null) {
      const [shape, word] = RESULT_SHOWN[a.result];
      return { shape, word, lines: [`${requested(a.mode)} · ${word} before this page loaded`] };
    }
    if (a.state === 'in-progress') return { shape: 'moving', word: 'in progress', lines: [`${requested(a.mode)} · in progress when this page loaded; Check again looks it up`] };
    return { shape: 'uncertain', word: 'uncertain', lines: [`${requested(a.mode)} · not settled when this page loaded`, 'it may or may not have been sent; the text stays'] };
  }
  // A refused retry leaves its attempt as it was.
  if (isUnsettled(a.state) && refusal(said)) return { shape: 'uncertain', word: 'uncertain', lines: [`this attempt was refused (${refusedWhy(said)}); the earlier send is still uncertain`] };
  return saidShown(said, a.mode);
}

function checkedLine(s: ThreadState, key: string): string | null {
  const c = s.checked?.key === key ? s.checked.note : null;
  if (c === null) return null;
  if (c.kind === 'no-record') return 'checked: no record of this send yet. One still on its way can arrive; Try again sends it at most once';
  if (c.kind === 'other-target') return 'checked: this message id belongs to another thread';
  return `couldn’t check: ${c.why}`;
}

export function Composer({
  id,
  listed,
  snapshotAt,
  composers,
  uploads,
}: {
  id: string;
  listed: boolean;
  /** When the open thread's latest snapshot arrived: each one re-reads the preview. */
  snapshotAt: number;
  composers: Composers;
  uploads: Uploads;
}) {
  const s = useSyncExternalStore(
    (onChange) => composers.subscribe(onChange),
    () => composers.get(id),
    () => composers.get(id),
  );
  const tray = useTray(uploads, threadTarget(id));
  const [previews, setPreviews] = useState<Previews>({ queue: null, queueAt: 0, steer: null, steerAt: 0 });
  const [note, setNote] = useState<string | null>(null);
  const [changed, setChanged] = useState(false);
  const [, setTick] = useState(0);
  const reading = useRef(false);
  const field = useRef<HTMLTextAreaElement>(null);
  const lastFocus = useRef<HTMLElement | null>(null);
  const stop = useRef(new AbortController());
  const lastKey = useRef<string | null>(null);

  useEffect(() => {
    const ctl = stop.current;
    return () => ctl.abort('stop');
  }, []);

  const newest = s.attempts.at(-1) ?? null;
  const anywayMode = !locked(s) && newest?.state === 'refused' && newest.result === 'duplicate' ? newest.mode : null;
  // Steer's preview is read when a running turn offers Steer, or a steer attempt may be sent again.
  const steerWanted = s.attempts.some((a) => a.mode === 'steer' && a.state !== 'done') || anywayMode === 'steer';

  // One preview at a time: on opening, on each new snapshot of the thread, and after each answer.
  useEffect(() => {
    if (reading.current) return;
    reading.current = true;
    const signal = stop.current.signal;
    void (async () => {
      const queue = await composers.preview(id, 'queue', signal);
      const queueAt = performance.now();
      const running = queue?.kind === 'ok' && queue.preview.effects !== null && 'known' in queue.preview.effects.effect && queue.preview.effects.effect.known.kind === 'queues_behind_turn';
      const steer = queue && (running || steerWanted) ? await composers.preview(id, 'steer', signal) : null;
      reading.current = false;
      if (queue && !signal.aborted) setPreviews({ queue, queueAt, steer, steerAt: performance.now() });
    })();
  }, [composers, id, snapshotAt, s.said, steerWanted]);

  // A focused control that goes away (its row settled, or the draft's buttons gave way to its attempts)
  // leaves focus in a fixed, named place: the message field. Never on the page's body.
  useEffect(() => {
    const was = lastFocus.current;
    if (was && !was.isConnected && (document.activeElement === null || document.activeElement === document.body)) {
      field.current?.focus();
      lastFocus.current = field.current;
    }
  });

  // The read's age, kept current.
  useEffect(() => {
    if (previews.queue === null) return;
    const t = window.setInterval(() => setTick((n) => n + 1), 1_000);
    return () => window.clearInterval(t);
  }, [previews.queue]);

  const isLocked = locked(s);
  const out = inFlight(s);
  const empty = s.draft.trim() === '';
  // What stops every send button alike; each adds its own preview's reason. An attachment not
  // ready stops them too .
  const off = !listed ? 'this thread is no longer listed on this page' : (composers.blocked(id) ?? composers.attachmentsOff(id));
  const gate = (label: string, mode: Mode) => gateOf(label, mode, previews, off);
  const idle = !empty && !out;
  const attached = tray.items.length > 0;
  const buttons = draftButtons(previews, attached);
  const steerOffered = previews.queue?.kind === 'ok' && previews.queue.preview.effects !== null && 'known' in previews.queue.preview.effects.effect && previews.queue.preview.effects.effect.known.kind === 'queues_behind_turn';
  const gates = buttons.map((b) => ({ ...b, gate: gate(b.label, b.mode) }));
  const previewState: PreviewState =
    previews.queue === null ? 'loading' : previews.queue.kind === 'failed' || previews.steer?.kind === 'failed' ? 'failed' : 'ready';


  const key = previewState === 'loading' ? null : gates.map((g) => `${g.gate.label}:${g.gate.effect ?? ''}`).join('|');
  useEffect(() => {
    if (key === null) return;
    if (lastKey.current !== null && lastKey.current !== key && s.draft.trim() !== '') setChanged(true);
    lastKey.current = key;
  }, [key, s.draft]);

  // The attempts' own controls: Try again an attempt with no answer; Send again after an uncertain one.
  const unknownAttempt = s.attempts.find((a) => a.state === 'unknown' && a.n === s.next[a.mode]) ?? null;
  const uncertain = unknownAttempt ? null : (s.attempts.findLast((a) => a.state === 'uncertain') ?? null);

  const send = (pressed: Press) => {
    setChanged(false);
    setNote(null);
    void composers.press(id, pressed);
  };

  // The keyboard's press (Ctrl+Enter or ⌘+Enter, once per key press): the same gate as the buttons.
  const onKeyDown = () => {
    if (isLocked) return;
    const first = idle ? gates.find((g) => g.gate.off === null) : undefined;
    if (first) send(first.gate.press);
  };

  const rows = s.attempts.map((a) => ({ a, shown: attemptShown(a, s.said.get(keyOf(a))) }));
  const signedOut = newest !== null && s.said.get(keyOf(newest))?.kind === 'signed-out';

  return (
    <section
      className="composer"
      data-part="composer"
      data-target={id}
      data-preview={previewState}
      aria-labelledby="composer-label"
      onFocus={(e) => {
        if (e.target instanceof HTMLElement) lastFocus.current = e.target;
      }}
    >
      <PromptBody
        fieldId="composer-text"
        labelId="composer-label"
        label="message"
        fieldRef={field}
        value={s.draft}
        readOnly={isLocked}
        describedBy={off ? 'composer-said composer-off' : 'composer-said'}
        note={note}
        notePart="composer-note"
        tray={{ uploads, target: threadTarget(id), locked: isLocked }}
        onChange={(text) => composers.edit(id, text)}
        onSubmit={onKeyDown}
        onRefuse={setNote}
      />
      {off ? (
        <p id="composer-off" className="why" data-part="send-off">
          can’t send: <bdi>{off}</bdi>
        </p>
      ) : null}
      {!isLocked && attached && steerOffered ? (
        <p className="effect" data-part="no-steer">
          {NO_STEER}
        </p>
      ) : null}
      {isLocked ? null : (
        <div className="acts" data-part="actions">
          {gates.map((g) => (
            <SendControl key={g.key} action={g.key} part="send" gate={g.gate} idle={idle} changed={changed} onPress={send} />
          ))}
          {anywayMode ? (
            <SendControl
              action="anyway"
              part="send-anyway"
              note="may duplicate the earlier send"
              gate={gate('Send anyway', anywayMode)}
              idle={idle}
              onPress={(p) => send({ ...p, acceptPossibleDuplicate: true })}
            />
          ) : null}
        </div>
      )}
      {/* Every attempt, in order, in one polite live region: a change in any row is announced, and focus never moves. */}
      <div id="composer-said" role="status" data-part="outcome" data-outcome={rows.at(-1)?.shown.shape ?? 'none'}>
        <ol className="attempts">
        {rows.map(({ a, shown }) => (
          <li key={keyOf(a)} className="said" data-part="attempt" data-mode={a.mode} data-outcome={shown.shape}>
            <span className={`oc ${shown.shape}`} data-part="outcome-marker">
              <span className="glyph" aria-hidden="true">
                {GLYPHS[shown.shape]}
              </span>{' '}
              <span className="word">{shown.word}</span>
            </span>
            {shown.lines.map((line, i) => (
              <p key={i} className="said-line" data-part="outcome-line">
                <bdi>{line}</bdi>
              </p>
            ))}
            {checkedLine(s, keyOf(a)) ? (
              <p className="said-line" data-part="checked">
                <bdi>{checkedLine(s, keyOf(a))}</bdi>
              </p>
            ) : null}
            {/* Kept mounted while its own request is out (disabled), so a focused control keeps focus. */}
            {isUnsettled(a.state) || a.state === 'checking' || (a.state === 'sending' && a.before !== null) ? (
              <div className="acts" data-part="unsettled">
                <div className="act">
                  <button
                    type="button"
                    className="btn"
                    data-part="check"
                    aria-disabled={out ? 'true' : undefined}
                    onClick={() => {
                      if (!out) void composers.check(id, keyOf(a));
                    }}
                  >
                    Check again
                  </button>
                  <p className="effect">looks it up; never sends</p>
                </div>
                {(a.state === 'unknown' || (a.state === 'sending' && a.before === 'unknown')) && a.n === s.next[a.mode] ? (
                  <SendControl
                    action="retry"
                    part="retry"
                    note="same message id: sent at most once"
                    gate={{ ...gate('Try again', a.mode), label: 'Try again' }}
                    idle={idle}
                    onPress={(p) => send({ ...p, mode: a.mode, acceptPossibleDuplicate: a.acceptPossibleDuplicate })}
                  />
                ) : null}
              </div>
            ) : null}
          </li>
        ))}
        </ol>
      </div>
      {isLocked ? (
        <div className="acts" data-part="unsettled">
          {uncertain ? (
            <SendControl
              action="again"
              part="send-again"
              note="a new send: may duplicate the earlier one"
              gate={gate('Send again', uncertain.mode)}
              idle={idle}
              onPress={(p) => send({ ...p, acceptPossibleDuplicate: true })}
            />
          ) : null}
          <div className="act">
            <button
              type="button"
              className="btn"
              data-part="forget"
              aria-disabled={out ? 'true' : undefined}
              onClick={() => {
                if (!out) composers.forget(id);
              }}
            >
              Forget it
            </button>
            <p className="effect">keeps the text; retires these attempts’ message ids</p>
          </div>
        </div>
      ) : null}
      {signedOut ? (
        <a className="btn" href="/">
          Sign in again
        </a>
      ) : null}
    </section>
  );
}
