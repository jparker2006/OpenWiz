

import { useEffect, useRef, useState, useSyncExternalStore } from 'react';

import { browserFetch, browserXhr } from '../api.ts';
import { guardDrops, Uploads } from '../attachments.ts';
import { browserStorage, Composers, newOpId } from '../composer.ts';
import { Launchers } from '../launcher.ts';
import type { Timers, Visibility } from '../poller.ts';
import { clockText } from '../present.ts';
import { TIMING } from '../timing.ts';
import { connection, isStale, listed, Viewer } from '../viewer.ts';
import type { Snapshot } from '../viewer.ts';
import { Detail } from './Detail.tsx';
import { Inventory, KeepPlace } from './Inventory.tsx';
import type { LaunchProject } from './Launcher.tsx';
import { ConnectionPill, Notice } from './Status.tsx';

const timers: Timers = {
  setTimeout: (fn, ms) => window.setTimeout(fn, ms),
  clearTimeout: (handle) => {
    if (typeof handle === 'number') window.clearTimeout(handle);
  },
};

const visibility: Visibility = {
  hidden: () => document.visibilityState === 'hidden',
  subscribe: (onChange) => {
    document.addEventListener('visibilitychange', onChange);
    return () => document.removeEventListener('visibilitychange', onChange);
  },
};

/** Re-renders when a snapshot turns stale, and when the page becomes visible again. */
function useStaleClock(last: Snapshot<unknown> | null): void {
  const [, setTick] = useState(0);
  useEffect(() => {
    if (!last) return;
    const ms = Math.min(last.receivedAt - performance.now(), last.receivedWall - Date.now()) + TIMING.staleAfterMs + 1;
    if (ms <= 0) return;
    const t = window.setTimeout(() => setTick((n) => n + 1), ms);
    return () => window.clearTimeout(t);
  }, [last]);
  useEffect(() => visibility.subscribe(() => setTick((n) => n + 1)), []);
}

/** How many snapshots the list has shown: a hook for the proofs (`data-snapshot-seq`). */
function useSnapshotSeq(last: unknown): number {
  const seen = useRef<{ last: unknown; n: number }>({ last: null, n: 0 });
  if (last && last !== seen.current.last) seen.current = { last, n: seen.current.n + 1 };
  return seen.current.n;
}

export function App() {
  const [viewer] = useState(() => new Viewer({ fetch: browserFetch, timers, visibility, now: () => performance.now(), wall: () => Date.now() }, TIMING));
  const state = useSyncExternalStore(
    (onChange) => viewer.subscribe(onChange),
    () => viewer.getState(),
    () => viewer.getState(),
  );
  // One tray per target , shared by the composer and the launcher through the one prompt body.
  const [uploads] = useState(
    () => new Uploads({ fetch: browserFetch, xhr: browserXhr, storage: browserStorage(), timers, urls: { create: (f) => URL.createObjectURL(f), revoke: (u) => URL.revokeObjectURL(u) } }),
  );
  const [composers] = useState(() => new Composers({ fetch: browserFetch, storage: browserStorage(), timers, newOpId: () => newOpId(), uploads }));
  const [launchers] = useState(() => new Launchers({ fetch: browserFetch, storage: browserStorage(), timers, newOpId: () => newOpId(), uploads }));
  const [launch, setLaunch] = useState<LaunchProject | null>(null);
  const [gapsOpen, setGapsOpen] = useState(false);
  const firstRows = useRef(false);
  const returnTo = useRef<string | null>(null);

  useEffect(() => {
    viewer.start();
    return () => viewer.stop();
  }, [viewer]);

  // A file dropped anywhere but a prompt body never opens in the browser; object URLs go with the page .
  useEffect(() => {
    const unguard = guardDrops(window);
    const leave = () => uploads.unload();
    window.addEventListener('pagehide', leave);
    return () => {
      unguard();
      window.removeEventListener('pagehide', leave);
    };
  }, [uploads]);

  const { list, detail } = state;
  useStaleClock(list.last);
  useStaleClock(detail?.last ?? null);
  const seq = useSnapshotSeq(list.last);
  const clocks = { mono: typeof performance === 'undefined' ? 0 : performance.now(), wall: Date.now() };
  const now = new Date(clocks.wall);
  const conn = connection(list, clocks, TIMING.staleAfterMs);
  const envelope = list.last?.envelope ?? null;
  const stale = list.last !== null && isStale(list.last, clocks, TIMING.staleAfterMs);
  const gaps = envelope?.gaps ?? [];

  useEffect(() => {
    if (!firstRows.current && envelope && envelope.data.sessions.length > 0) {
      firstRows.current = true;
      performance.mark('first-rows');
    }
  }, [envelope]);

  const select = (id: string) => {
    setLaunch(null);
    returnTo.current = id;
    viewer.select(id);
  };
  const openLaunch = (project: LaunchProject) => {
    viewer.select(null);
    returnTo.current = null;
    setLaunch(project);
  };
  const closeDetail = () => {
    // Focus goes back where the panel was opened from: the session's row, or the project's New thread.
    const id = launch ? launch.id : returnTo.current;
    const from = (el: HTMLElement) => (launch ? el.dataset['newThread'] : el.dataset['sessionId']);
    viewer.select(null);
    setLaunch(null);
    returnTo.current = null;
    for (const el of document.querySelectorAll<HTMLElement>(launch ? 'main [data-new-thread]' : 'main [data-session-id]')) {
      if (from(el) === id) {
        el.focus();
        break;
      }
    }
  };
  const isListed = (id: string) => envelope?.data.sessions.some((row) => row.id === id) ?? false;

  return (
    <div className={`app${detail || launch ? ' panel-open' : ''}`}>
      <header className="top">
        <span className="logo-slot" aria-hidden="true">
          logo slot
        </span>
        <p className="wordmark">
          <b>OpenWiz</b> <span>web</span>
        </p>
        <ConnectionPill connection={conn} />
      </header>
      <main data-snapshot-seq={seq}>
        <header className="page-head">
          <h1 className="disp m">Sessions</h1>
          {envelope ? (
            <p className="inv">
              {gaps.length > 0 ? (
                <button type="button" className="cov" aria-expanded={gapsOpen} onClick={() => setGapsOpen((o) => !o)}>
                  <span className={envelope.coverage.state === 'complete' ? undefined : 'tag dashed'} data-part="coverage">
                    {envelope.coverage.state === 'complete' ? 'complete' : `partial · ${gaps.length} ${gaps.length === 1 ? 'gap' : 'gaps'}`}
                  </span>
                  <span aria-hidden="true">{gapsOpen ? '▴' : '▾'}</span>
                </button>
              ) : envelope.coverage.state === 'complete' ? (
                <span data-part="coverage">complete</span>
              ) : (
                <span className="tag dashed" data-part="coverage">
                  partial · 0 gaps
                </span>
              )}
              <span aria-hidden="true">·</span>
              <span data-part="snapshot">snapshot {clockText(envelope.freshness.snapshot_at, now)}</span>
              {stale ? (
                <span className="mod" data-part="stale">
                  stale
                </span>
              ) : null}
            </p>
          ) : null}
          {envelope && gapsOpen && gaps.length > 0 ? (
            <ul className="gaps" data-part="gaps">
              {gaps.map((g, i) => (
                <li key={i}>
                  <bdi>{`${g.kind}${g.subject === null ? '' : ` · ${g.subject}`} · ${g.detail}`}</bdi>
                </li>
              ))}
            </ul>
          ) : null}
        </header>
        <KeepPlace snapshot={list}>
          {list.failure ? <Notice failure={list.failure} lastGoodAt={envelope?.freshness.snapshot_at ?? null} now={now} /> : null}
          {envelope ? (
            <Inventory
              envelope={envelope}
              previousMatched={list.previousMatched}
              selectedId={detail?.id ?? null}
              launchId={launch?.id ?? null}
              fresh={!stale && list.failure === null}
              now={now}
              onSelect={select}
              onNewThread={openLaunch}
              onPage={(offset) => {
                viewer.goTo(offset);
                window.scrollTo(0, 0);
              }}
            />
          ) : list.failure ? null : (
            <p className="meta">reading…</p>
          )}
        </KeepPlace>
      </main>
      <Detail
        detail={detail}
        launch={launch ? { project: launch, launchers, uploads, listed: isListed, snapshotAt: list.last?.receivedAt ?? 0, onShowThread: select } : null}
        listed={listed(state)}
        stale={detail?.last ? isStale(detail.last, clocks, TIMING.staleAfterMs) : false}
        now={now}
        composers={composers}
        uploads={uploads}
        onClose={closeDetail}
      />
    </div>
  );
}
