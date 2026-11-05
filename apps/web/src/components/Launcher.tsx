

import { useEffect, useRef, useState, useSyncExternalStore } from 'react';

import type { FactString } from '../envelope.ts';
import { busy, effortsOf, locked, modelsOf, offeredHarness } from '../launcher.ts';
import type { Field, Launchers, LaunchState, OptionsSaid, PreviewSaid, Said } from '../launcher.ts';
import type { ThreadBody } from '../api.ts';
import type { Report, StartView } from '../newthread.ts';
import { ageText, clockText, factText } from '../present.ts';
import { launchTarget } from '../attachments.ts';
import type { Uploads } from '../attachments.ts';
import { useTray } from './AttachmentTray.tsx';
import { PromptBody } from './PromptBody.tsx';

/** The project the launcher creates in, as its heading shows it. */
export type LaunchProject = { id: string; name: FactString; suffix: string | null };

type Shape = 'sent' | 'failed' | 'uncertain' | 'not-sent' | 'moving';

const GLYPHS: Record<Shape, string> = { sent: '[✓]', failed: '[x]', uncertain: '[?]', 'not-sent': '[-]', moving: '[~]' };

type Shown = { shape: Shape; word: string; lines: string[] };

/** The core's status word, as reported. */
const statusWord = (r: Report): string => r.status.replace('_', ' ');

function reportShown(r: Report, read: boolean): Shown {
  const lines = [r.summary];
  if (read) lines.push('read from its record; nothing was created by the read');
  else if (r.result.kind === 'outcome' && r.result.replayed) lines.push('replayed: this op id was used before; this is its recorded outcome, and nothing was called');
  if (r.result.kind === 'conflicting_reuse') lines.push(`this op id was used for a different ${r.result.differs.join(', ')}; nothing was created with this press`);
  switch (r.status) {
    case 'created':
      return { shape: 'sent', word: statusWord(r), lines };
    case 'failed':
      return { shape: 'failed', word: statusWord(r), lines };
    case 'uncertain':
      return { shape: 'uncertain', word: statusWord(r), lines: [...lines, 'nothing is retried, by itself or by OpenWiz; resolve it in CloudRoom'] };
    case 'in_progress':
      return { shape: 'moving', word: statusWord(r), lines: [...lines, 'another tab or process is creating with this op id'] };
    case 'refused':
    case 'conflicting_reuse':
      return { shape: 'not-sent', word: statusWord(r), lines };
  }
}

function saidShown(said: Said): Shown {
  switch (said.kind) {
    case 'report':
      return reportShown(said.report, said.read);
    case 'signed-out':
      return { shape: 'not-sent', word: 'signed out', lines: ['nothing was created; sign in again, then start'] };
    case 'not-created':
      return { shape: 'not-sent', word: 'not created', lines: [said.error ? `${said.error.kind}: ${said.error.message}` : `HTTP ${said.status}`] };
    case 'unknown': {
      const why =
        said.cause === 'network'
          ? 'the connection was lost before an answer'
          : said.cause === 'timeout'
            ? 'no answer within 45 s'
            : said.cause === 'body'
              ? 'the answer was not this create’s report'
              : said.cause === 'reload'
                ? 'the page reloaded during the create'
                : `HTTP ${said.status ?? ''}${said.error ? `: ${said.error.kind}: ${said.error.message}` : ''}`;
      return { shape: 'uncertain', word: 'uncertain', lines: [`Start thread pressed · ${why}`, 'it may or may not have been created; the prompt, settings and op id stay'] };
    }
  }
}

/** The draft's attempt, as it stands. */
function attemptShown(s: LaunchState): Shown | null {
  const p = s.pending;
  if (p?.state === 'creating') return { shape: 'moving', word: 'creating', lines: ['Start thread pressed · waiting for the answer'] };
  if (p?.state === 'checking') return { shape: 'moving', word: 'checking', lines: ['reading this create’s record; nothing is created'] };
  const shown: Shown | null = s.said
    ? saidShown(s.said)
    : p
      ? { shape: p.state === 'in-progress' ? 'moving' : 'uncertain', word: p.state === 'in-progress' ? 'in progress' : 'uncertain', lines: ['not settled when this page loaded; Refresh status reads its record', 'it may or may not have been created; the prompt, settings and op id stay'] }
      : null;
  // A refused Try again: the earlier answer stays as it was, and the refusal is a line of its own.
  if (shown && s.retry) return { ...shown, lines: [...shown.lines, `this Try again was refused (${s.retry}); the earlier create is still uncertain`] };
  return shown;
}

function stageLine(start: StartView | null, now: Date): string {
  if (!start) return 'start: not read yet';
  const stage = start.stage;
  const error = stage.kind === 'start_failed' ? ` · last error: ${factText(stage.last_error)}` : '';
  return `start: ${stage.text}${error} · read ${clockText(start.read_at, now)}`;
}

/** The new thread's details: created and its start, apart; requested beside reported (U10, U8). */
function CreatedDetails({ project, launchers, listed, onShowThread }: { project: LaunchProject; launchers: Launchers; listed: (id: string) => boolean; onShowThread: (id: string) => void }) {
  const s = useSyncExternalStore(
    (onChange) => launchers.subscribe(onChange),
    () => launchers.get(project.id),
    () => launchers.get(project.id),
  );
  const c = s.created;
  if (!c || c.report.result.kind !== 'outcome' || c.report.result.outcome.kind !== 'created') return null;
  const outcome = c.report.result.outcome;
  const now = new Date();
  const req = c.report.requested;
  const reported = c.start?.reported ?? null;
  return (
    <section className="launcher" data-part="created" data-thread-id={outcome.thread_id} aria-labelledby="created-title">
      <h2 id="created-title" className="disp s">
        New thread
      </h2>
      <div role="status" data-part="launch-outcome" data-outcome="created">
        <span className="oc sent" data-part="outcome-marker">
          <span className="glyph" aria-hidden="true">
            {GLYPHS.sent}
          </span>{' '}
          <span className="word">created</span>
        </span>
        <p className="said-line" data-part="created-line">
          <bdi>{outcome.text}</bdi>
        </p>
        <p className="said-line" data-part="start-line">
          <bdi>{stageLine(c.start, now)}</bdi>
        </p>
        {c.report.attachments && c.report.attachments.length > 0 ? (
          <p className="said-line" data-part="handed-over">
            {`${c.report.attachments.length} ${c.report.attachments.length === 1 ? 'attachment' : 'attachments'} handed to CloudRoom`}
          </p>
        ) : null}
      </div>
      <dl className="kv">
        <dt>thread</dt>
        <dd data-field="thread">
          <bdi>{outcome.thread_id}</bdi>
        </dd>
        <dt>project</dt>
        <dd data-field="project">
          <bdi>{factText(project.name)}</bdi>
        </dd>
        <dt>requested</dt>
        <dd data-field="requested">
          <bdi>{`harness ${req.harness} · model ${req.model} · effort ${req.effort}`}</bdi>
        </dd>
        <dt>reported</dt>
        <dd data-field="reported">
          {reported ? (
            <>
              <span className="meta" data-part="reported-label">
                <bdi>{reported.label}</bdi>
              </span>
              {[reported.provider, reported.model, reported.effort].map((r, i) => (
                <span key={i} className={`said-line${'known' in r.differs_from_requested && r.differs_from_requested.known ? ' differs' : ''}`} data-part="reported-setting">
                  <bdi>{r.text}</bdi>
                </span>
              ))}
            </>
          ) : (
            'not read yet'
          )}
        </dd>
      </dl>
      <div className="acts">
        <div className="act">
          {listed(outcome.thread_id) ? (
            <button type="button" className="btn" data-part="show-thread" onClick={() => onShowThread(outcome.thread_id)}>
              Show thread
            </button>
          ) : null}
          <p className="effect">{listed(outcome.thread_id) ? 'its details, and its composer' : 'it appears in the list at the next refresh'}</p>
        </div>
        <div className="act">
          <button type="button" className="btn" data-part="refresh-start" onClick={() => void launchers.readStart(project.id)}>
            Refresh status
          </button>
          <p className="effect">reads its start; never creates</p>
        </div>
        <div className="act">
          <button type="button" className="btn" data-part="another" onClick={() => launchers.closeCreated(project.id)}>
            New thread
          </button>
          <p className="effect">another thread in this project</p>
        </div>
      </div>
    </section>
  );
}

/** One of the three settings: a large native control, its choice in words, only offered values. */
function Pick({
  field,
  label,
  value,
  values,
  unavailable,
  disabledWhy,
  readOnly,
  unlisted,
  onPick,
}: {
  field: Field;
  label: string;
  value: string;
  values: string[];
  unavailable: { value: string; text: string }[];
  disabledWhy: string | null;
  readOnly: boolean;
  /** Why a chosen value the current options don't list is still shown (the options weren't read, or no longer offer it). */
  unlisted: string;
  onPick: (value: string) => void;
}) {
  const id = `launch-${field}`;
  const placeholder = disabledWhy ?? `choose ${field === 'effort' ? 'an' : 'a'} ${field}`;
  // A chosen value stays visible in its control even when the current options don't list it:
  // an unresolved attempt's settings are fixed, and what is sent is always what is shown.
  const extra = value !== '' && !values.includes(value) ? value : null;
  return (
    <div className="pick" data-part="pick" data-field={field} data-value={value}>
      <label htmlFor={id} className="pick-label">
        {label}
      </label>
      <select
        id={id}
        className="pick-select"
        value={value}
        aria-disabled={readOnly || disabledWhy !== null ? 'true' : undefined}
        onChange={(e) => {
          if (!readOnly && disabledWhy === null) onPick(e.target.value);
        }}
      >
        <option value="">{placeholder}</option>
        {extra === null ? null : (
          <option value={extra} data-part="unlisted">
            {`${extra} · ${unlisted}`}
          </option>
        )}
        {values.map((v) => (
          <option key={v} value={v}>
            {v}
          </option>
        ))}
        {unavailable.map((u) => (
          <option key={u.value} value={`unavailable:${u.value}`} disabled>
            {`${u.value} · ${u.text}`}
          </option>
        ))}
      </select>
    </div>
  );
}

export function Launcher({
  project,
  launchers,
  uploads,
  listed,
  snapshotAt,
  onShowThread,
}: {
  project: LaunchProject;
  launchers: Launchers;
  uploads: Uploads;
  /** Whether a thread id is on the list's latest page: a new thread can be shown once it is. */
  listed: (id: string) => boolean;
  /** When the list's latest snapshot arrived: each one may read the status again (bounded, read-only). */
  snapshotAt: number;
  onShowThread: (id: string) => void;
}) {
  const s = useSyncExternalStore(
    (onChange) => launchers.subscribe(onChange),
    () => launchers.get(project.id),
    () => launchers.get(project.id),
  );
  const tray = useTray(uploads, launchTarget(project.id));
  const [options, setOptions] = useState<{ said: OptionsSaid; at: number } | null>(null);
  const [preview, setPreview] = useState<{ body: ThreadBody; said: PreviewSaid; at: number } | null>(null);
  const [optionsRead, setOptionsRead] = useState(0);
  const [, setTick] = useState(0);
  const stop = useRef(new AbortController());
  const field = useRef<HTMLTextAreaElement>(null);
  const lastFocus = useRef<HTMLElement | null>(null);

  useEffect(() => {
    const ctl = stop.current;
    return () => ctl.abort('stop');
  }, []);

  // The options: on opening, and when the owner (or a "not offered" refusal) asks again. A read, never a create.
  useEffect(() => {
    const signal = stop.current.signal;
    void (async () => {
      const said = await launchers.options(signal);
      if (said && !signal.aborted) {
        setOptions({ said, at: performance.now() });
        // Only options read say what is offered: a failed read empties nothing (it is shown on its own).
        if (said.kind === 'ok') launchers.reconcile(project.id, said.options);
      }
    })();
  }, [launchers, project.id, optionsRead]);

  // A refusal for a choice the options don't offer reads them again, visibly.
  const refusedFor = s.said?.kind === 'report' && s.said.report.result.kind === 'outcome' && s.said.report.result.outcome.kind === 'refused' ? s.said.report.result.outcome.reason : null;
  useEffect(() => {
    if (refusedFor && ['no_options', 'harness_not_offered', 'model_not_offered', 'effort_not_offered'].includes(refusedFor)) setOptionsRead((n) => n + 1);
  }, [refusedFor, s.n]);

  const opts = options?.said.kind === 'ok' ? options.said.options : null;
  const complete = s.choice.harness !== '' && s.choice.model !== '' && s.choice.effort !== '';
  const hasText = s.draft.trim() !== '';
  const isLocked = locked(s);

  const ids = launchers.attachments(project.id);
  const trayOff = launchers.attachmentsOff(project.id);
  const previewKey =
    complete && hasText && !isLocked && s.root !== null && trayOff === null ? `${s.choice.harness}|${s.choice.model}|${s.choice.effort}|${s.root}|${s.n}|${optionsRead}|${ids.join(',')}` : null;
  const previewFailed = preview?.said.kind === 'failed';

  // What a create would do: the core's preview, once the settings are chosen and there is a prompt, again when
  // they change, and again after a failed read when the prompt changes. A read, never a create.
  useEffect(() => {
    if (previewKey === null) return;
    const signal = stop.current.signal;
    const t = window.setTimeout(() => {
      void (async () => {
        const read = await launchers.preview(project.id, signal);
        if (read && !signal.aborted) setPreview({ ...read, at: performance.now() });
      })();
    }, 400);
    return () => window.clearTimeout(t);
  }, [launchers, project.id, previewKey, previewFailed ? s.draft : null]);

  // 0.0.2's refresh: each list snapshot may read the record of an unresolved attempt, or the created thread's
  // start, at most ten times. Reads only: never a create.
  const first = useRef(true);
  useEffect(() => {
    if (first.current) {
      first.current = false;
      return;
    }
    if (launchers.get(project.id).created) void launchers.followStart(project.id);
    else void launchers.follow(project.id);
  }, [launchers, project.id, snapshotAt]);

  // The reads' ages, kept current.
  useEffect(() => {
    const t = window.setInterval(() => setTick((n) => n + 1), 1_000);
    return () => window.clearInterval(t);
  }, []);

  // A focused control that goes away (Start thread giving way to Refresh status) leaves focus in a fixed,
  // named place: the prompt. Never on the page's body (as the composer's rule 25).
  useEffect(() => {
    const was = lastFocus.current;
    if (was && !was.isConnected && (document.activeElement === null || document.activeElement === document.body)) {
      field.current?.focus();
      lastFocus.current = field.current;
    }
  });

  if (s.created) return <CreatedDetails project={project} launchers={launchers} listed={listed} onShowThread={onShowThread} />;

  const out = busy(s);
  const blocked = launchers.blocked(project.id);
  const forThis =
    preview !== null &&
    preview.body.op_id === `${s.root}-c${s.n}` &&
    preview.body.harness === s.choice.harness &&
    preview.body.model === s.choice.model &&
    preview.body.effort === s.choice.effort &&
    preview.body.attachments.join(',') === ids.join(',');
  const shownPreview = forThis ? preview : null;
  const verdict = shownPreview?.said.kind === 'ok' ? shownPreview.said.preview.verdict : null;
  const effects = shownPreview?.said.kind === 'ok' ? shownPreview.said.preview.effects : null;

  /** Why Start thread can't create now, in words; null when it can. */
  const gate = ((): string | null => {
    if (blocked) return blocked;
    if (!options) return 'reading the options';
    if (options.said.kind === 'failed') return `the options couldn’t be read (${options.said.why})`;
    if (options.said.options.state.kind === 'no_options') return options.said.options.state.text;
    if (s.choice.harness === '') return 'choose a harness';
    if (s.choice.model === '') return 'choose a model';
    if (s.choice.effort === '') return 'choose an effort';
    if (!hasText) return 'write a prompt';
    if (trayOff) return trayOff;
    if (!shownPreview) return 'reading what Start thread would do';
    if (shownPreview.said.kind === 'failed') return `what Start thread would do couldn’t be read (${shownPreview.said.why})`;
    if (verdict?.kind === 'stopped') return verdict.outcome.text;
    return null;
  })();
  // Try again after no answer: the same attempt, the same op id; created at most once. Only storage gates it.
  const retry = s.pending?.state === 'unknown' || (s.pending?.state === 'creating' && s.pending.before === 'unknown');
  const off = retry ? blocked : gate;
  const canPress = !out && off === null && (s.pending === null || retry);
  const onStart = () => {
    if (canPress) void launchers.create(project.id);
  };
  const shown = attemptShown(s);
  const unresolved = s.pending !== null && s.pending.state !== 'creating';
  const pick = (f: Field) => (value: string) => launchers.choose(project.id, f, value, opts);
  const harnesses = opts?.harnesses.map((h) => h.harness) ?? [];
  const harnessOffered = offeredHarness(opts, s.choice.harness) !== null;
  const unlisted = opts ? (isLocked ? 'requested; not offered now' : 'not offered now') : 'options not read';

  return (
    <section
      className="launcher"
      data-part="launcher"
      data-project-id={project.id}
      data-preview={shownPreview ? shownPreview.said.kind : 'none'}
      aria-labelledby="launch-title"
      onFocus={(e) => {
        if (e.target instanceof HTMLElement) lastFocus.current = e.target;
      }}
    >
      <h2 id="launch-title" className="disp s">
        New thread
      </h2>
      <dl className="kv">
        <dt>project</dt>
        <dd data-field="project">
          <bdi>{factText(project.name)}</bdi>
          {project.suffix === null ? null : <span className="gid">{` · ${project.suffix}`}</span>}
        </dd>
        <dt>where</dt>
        <dd data-field="where">Cloud</dd>
      </dl>
      <p className="sec">requested settings</p>
      {options?.said.kind === 'ok' ? (
        <p className="meta" data-part="options-read">
          {`options read ${clockText(options.said.options.read_at, new Date())} · ${options.said.options.label}`}
        </p>
      ) : options?.said.kind === 'failed' ? (
        <p className="why" data-part="options-failed">
          options couldn’t be read: <bdi>{options.said.why}</bdi>
        </p>
      ) : (
        <p className="meta">reading the options…</p>
      )}
      {opts?.state.kind === 'no_options' ? (
        <p className="tag dashed" data-part="no-options">
          <bdi>{opts.state.text}</bdi>
        </p>
      ) : null}
      <div className="picks">
        <Pick
          field="harness"
          label="harness"
          value={s.choice.harness}
          values={harnesses}
          unavailable={opts?.unavailable.map((u) => ({ value: u.harness, text: u.text })) ?? []}
          disabledWhy={opts ? null : 'no options yet'}
          readOnly={isLocked}
          unlisted={unlisted}
          onPick={pick('harness')}
        />
        <Pick
          field="model"
          label="model"
          value={s.choice.model}
          values={modelsOf(opts, s.choice.harness)}
          unavailable={[]}
          disabledWhy={harnessOffered ? null : 'choose a harness first'}
          readOnly={isLocked}
          unlisted={unlisted}
          onPick={pick('model')}
        />
        <Pick
          field="effort"
          label="effort"
          value={s.choice.effort}
          values={effortsOf(opts, s.choice.harness, s.choice.model)}
          unavailable={[]}
          disabledWhy={s.choice.model !== '' ? null : 'choose a model first'}
          readOnly={isLocked}
          unlisted={unlisted}
          onPick={pick('effort')}
        />
      </div>
      {isLocked ? (
        <p className="meta" data-part="picks-fixed">
          requested for this attempt; fixed while it may have created a thread
        </p>
      ) : null}
      <div className="act">
        <button type="button" className="btn" data-part="refresh-options" aria-disabled={isLocked ? 'true' : undefined} onClick={() => !isLocked && setOptionsRead((n) => n + 1)}>
          Refresh options
        </button>
        <p className="effect">reads them again from CloudRoom</p>
      </div>
      {s.note ? (
        <p className="tag dashed" data-part="launch-note">
          <bdi>{s.note}</bdi>
        </p>
      ) : null}
      <PromptBody
        fieldId="launch-text"
        labelId="launch-label"
        label="prompt"
        fieldRef={field}
        value={s.draft}
        readOnly={isLocked}
        describedBy="launch-said launch-off"
        note={null}
        notePart="launch-prompt-note"
        tray={{ uploads, target: launchTarget(project.id), locked: isLocked }}
        onChange={(text) => launchers.edit(project.id, text)}
        onSubmit={onStart}
        onRefuse={(words) => launchers.note(project.id, words)}
      />
      <p className="sec">what Start thread does</p>
      {effects ? (
        <>
          <ul className="effects" data-part="effects">
            {effects.effects.map((e) => (
              <li key={e.kind} data-effect={e.kind}>
                <bdi>{e.text}</bdi>
              </li>
            ))}
          </ul>
          <p className="meta" data-part="effects-age">
            {`effects read ${ageText(Math.max(0, performance.now() - (shownPreview?.at ?? 0)))} ago`}
          </p>
        </>
      ) : (
        <p className="meta" data-part="effects-none">
          {verdict?.kind === 'stopped' ? 'a create would not go ahead' : 'shown once the settings are chosen and there is a prompt'}
        </p>
      )}
      {/* Kept mounted (disabled) while its own request is out, so a focused Start thread keeps focus. */}
      {isLocked && !retry && !out ? null : (
        <div className="acts" data-part="launch-actions">
          <div className="act">
            <button type="button" className="btn" data-part="start" aria-disabled={canPress ? undefined : 'true'} onClick={onStart}>
              {retry ? 'Try again' : 'Start thread'}
            </button>
            <p id="launch-off" className="effect" data-part="start-effect">
              {off ? (
                <span className="why">
                  can’t start: <bdi>{off}</bdi>
                </span>
              ) : retry ? (
                'the same op id: created at most once'
              ) : (
                'creates one thread, as above'
              )}
            </p>
          </div>
        </div>
      )}
      <div id="launch-said" role="status" data-part="launch-outcome" data-outcome={shown?.shape ?? 'none'}>
        {shown ? (
          <div className="said" data-part="launch-attempt">
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
            {s.observations ? (
              <p className="said-line" data-part="observations">
                <bdi>{`${s.observations.threads.length} ${s.observations.threads.length === 1 ? 'thread' : 'threads'} seen in this project since · ${s.observations.text}`}</bdi>
              </p>
            ) : null}
            {s.checked ? (
              <p className="said-line" data-part="checked">
                <bdi>{s.checked}</bdi>
              </p>
            ) : null}
          </div>
        ) : null}
      </div>
      {unresolved || s.pending?.state === 'checking' ? (
        <div className="acts" data-part="unresolved">
          <div className="act">
            <button type="button" className="btn" data-part="refresh-status" aria-disabled={out ? 'true' : undefined} onClick={() => !out && void launchers.check(project.id)}>
              Refresh status
            </button>
            <p className="effect">reads this create’s record; never creates</p>
          </div>
          <div className="act">
            <button type="button" className="btn" data-part="start-over" aria-disabled={out ? 'true' : undefined} onClick={() => !out && launchers.startOver(project.id)}>
              Start over
            </button>
            <p className="effect">a new attempt: may make a second thread</p>
          </div>
        </div>
      ) : null}
    </section>
  );
}
