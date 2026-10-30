// The details of one session, as `show` returns them: a modal bottom sheet on
// phones, a side panel on desktop (≥ 900 px) (V9). Read-only: the only
// control is Close.

import { useEffect, useRef, useSyncExternalStore } from 'react';

import type { Envelope, SessionView } from '../envelope.ts';
import { ageText, boundedText, clockText, factText, unknownText } from '../present.ts';
import type { DetailState } from '../viewer.ts';
import { Marker, Notice } from './Status.tsx';

const WIDE = '(min-width: 900px)';

function useWide(): boolean {
  return useSyncExternalStore(
    (onChange) => {
      const q = window.matchMedia(WIDE);
      q.addEventListener('change', onChange);
      return () => q.removeEventListener('change', onChange);
    },
    () => window.matchMedia(WIDE).matches,
    () => false,
  );
}

function Fields({ envelope, stale, now }: { envelope: Envelope<SessionView>; stale: boolean; now: Date }) {
  const { session: s, final_output: out, reported_error: err } = envelope.data;
  const location = !('known' in s.location)
    ? unknownText(s.location.unknown)
    : s.location.known.kind === 'cloud'
      ? 'cloud'
      : `local on ${factText(s.location.known.machine)}`;
  const purpose = !('known' in s.recap)
    ? unknownText(s.recap.unknown)
    : s.recap.known.kind === 'no_recap_yet'
      ? 'no recap yet'
      : boundedText(s.recap.known.text, s.recap.known.truncated);
  const config = s.reported_configuration;
  return (
    <>
      <h2 id="detail-title" className="disp s">
        <bdi>{factText(s.title)}</bdi>
      </h2>
      <dl className="kv">
        <dt>status</dt>
        <dd data-field="status">
          <Marker status={s.status} />
        </dd>
        <dt>{s.group.kind === 'project' ? 'project' : 'ungrouped'}</dt>
        <dd data-field="group">
          <bdi>{s.group.kind === 'project' ? factText(s.group.name) : s.group.reason}</bdi>
        </dd>
        <dt>location</dt>
        <dd data-field="location">
          <bdi>{location}</bdi>
        </dd>
        <dt>purpose, as reported</dt>
        <dd data-field="purpose">
          <bdi>{purpose}</bdi>
        </dd>
        <dt>reported configuration</dt>
        <dd data-field="configuration">
          model <bdi>{factText(config.model)}</bdi> · effort <bdi>{factText(config.effort)}</bdi>
        </dd>
        <dt>snapshot</dt>
        <dd data-field="snapshot">
          {clockText(envelope.freshness.snapshot_at, now)}
          {stale ? (
            <span className="mod" data-part="detail-stale">
              {' '}
              stale
            </span>
          ) : null}
        </dd>
        <dt>age</dt>
        <dd data-field="age">{'known' in s.age_ms ? ageText(s.age_ms.known) : unknownText(s.age_ms.unknown)}</dd>
        <dt>coverage</dt>
        <dd data-field="coverage">
          <span data-part="detail-coverage" className={envelope.coverage.state === 'partial' ? 'tag dashed' : undefined}>
            {envelope.coverage.state}
          </span>
        </dd>
        <dt>reported error</dt>
        <dd data-field="reported-error">
          <bdi>{!('known' in err) ? unknownText(err.unknown) : err.known === null ? 'none' : boundedText(err.known.text, err.known.truncated)}</bdi>
        </dd>
      </dl>
      {envelope.gaps.length > 0 ? (
        <>
          <p className="sec">
            {envelope.gaps.length} {envelope.gaps.length === 1 ? 'gap' : 'gaps'}
          </p>
          <ul className="gaps" data-part="detail-gaps">
            {envelope.gaps.map((g, i) => (
              <li key={i}>
                <bdi>{`${g.kind}${g.subject === null ? '' : ` · ${g.subject}`} · ${g.detail}`}</bdi>
              </li>
            ))}
          </ul>
        </>
      ) : null}
      <p className="sec">final output</p>
      <div data-field="final-output">
        {!('known' in out) ? (
          unknownText(out.unknown)
        ) : out.known.kind === 'no_final_output_yet' ? (
          'no final output yet'
        ) : (
          <>
            <pre className="out" dir="auto">
              {boundedText(out.known.text, out.known.truncated)}
            </pre>
            {out.known.truncated ? (
              <span className="mod" data-part="final-output-truncated">
                truncated
              </span>
            ) : null}
          </>
        )}
      </div>
    </>
  );
}

export function Detail({
  detail,
  listed,
  stale,
  now,
  onClose,
}: {
  detail: DetailState | null;
  listed: boolean;
  stale: boolean;
  now: Date;
  onClose: () => void;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const closeButton = useRef<HTMLButtonElement>(null);
  const switching = useRef(false);
  const loadedFor = useRef<string | null>(null);
  const wide = useWide();
  const id = detail?.id ?? null;

  useEffect(() => {
    const d = ref.current;
    if (!d) return;
    if (id === null) {
      if (d.open) d.close();
      return;
    }
    if (d.open && d.matches(':modal') === !wide) return;
    if (d.open) {
      switching.current = true;
      d.close();
    }
    if (wide) d.show();
    else d.showModal();
    closeButton.current?.focus();
    performance.mark('detail-shown');
  }, [id, wide]);

  useEffect(() => {
    if (id === null) loadedFor.current = null;
    else if (detail?.last && loadedFor.current !== id) {
      loadedFor.current = id;
      performance.mark('detail-loaded');
    }
  }, [id, detail?.last]);

  return (
    <dialog
      ref={ref}
      className="sheet"
      data-part="detail"
      data-session-id={id ?? undefined}
      aria-labelledby={detail?.last ? 'detail-title' : undefined}
      aria-label={detail?.last ? undefined : 'Session details'}
      onClose={() => {
        if (switching.current) {
          switching.current = false;
          return;
        }
        onClose();
      }}
      onKeyDown={(e) => {
        // A modal sheet closes on Escape by itself; the side panel is not modal.
        if (e.key === 'Escape' && !e.currentTarget.matches(':modal')) {
          e.preventDefault();
          e.currentTarget.close();
        }
      }}
    >
      <div className="sheet-grab" aria-hidden="true" />
      <header className="sheet-head">
        <p className="sheet-cmd">show · read-only</p>
        <button ref={closeButton} type="button" className="sheet-x" aria-label="Close" onClick={() => ref.current?.close()}>
          ✕
        </button>
      </header>
      {detail ? (
        <>
          {!listed ? (
            <p className="tag dashed" data-part="no-longer-listed">
              no longer listed on this page
            </p>
          ) : null}
          {detail.failure ? <Notice failure={detail.failure} lastGoodAt={detail.last?.envelope.freshness.snapshot_at ?? null} now={now} /> : null}
          {detail.last ? <Fields envelope={detail.last.envelope} stale={stale} now={now} /> : detail.failure ? null : <p className="meta">reading…</p>}
        </>
      ) : null}
    </dialog>
  );
}
