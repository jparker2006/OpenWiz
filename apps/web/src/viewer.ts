// The page's presentation state, and nothing else: the current page, the last
// good snapshot (with when it arrived, on the device's monotonic clock), the
// latest failure, and the selected session with its detail. Every rule stays
// in the envelope.

import { inventoryUrl, read, serverDone, sessionUrl } from './api.ts';
import type { Failure, FetchLike, Outcome } from './api.ts';
import { parseInventory, parseSessionView } from './envelope.ts';
import type { Envelope, Inventory, SessionView } from './envelope.ts';
import { Lane, Poller } from './poller.ts';
import type { Settled, Timers, Visibility } from './poller.ts';
import type { Timing } from './timing.ts';

/** An envelope, with when it arrived on the device's monotonic and wall clocks. */
export type Snapshot<D> = { envelope: Envelope<D>; receivedAt: number; receivedWall: number };

export type ListState = {
  offset: number;
  last: Snapshot<Inventory> | null;
  failure: Failure | null;
  /**
   * The `matched` count of the page this one was reached from, so a change
   * between the two reads can be shown: offset paging over a live list can
   * miss or repeat a session.
   */
  previousMatched: number | null;
};

export type DetailState = {
  id: string;
  last: Snapshot<SessionView> | null;
  failure: Failure | null;
};

export type ViewState = {
  list: ListState;
  detail: DetailState | null;
};

export type ViewerDeps = {
  fetch: FetchLike;
  timers: Timers;
  visibility: Visibility;
  /** Monotonic milliseconds (`performance.now()` in the browser). */
  now(): number;
  /** Wall-clock milliseconds (`Date.now()`): it keeps counting while a phone sleeps. */
  wall(): number;
};

/** The device's two clocks, read together. */
export type Now = { mono: number; wall: number };

/** The connection word the header shows. Each has its own shape (V6). */
export type Connection = 'reading' | 'live' | 'stale' | 'disconnected' | 'signed-out' | 'busy' | 'error';

/**
 * Stale when either device clock says the snapshot is older than the
 * threshold since its response arrived: the monotonic one (immune to the wall
 * clock being set) or the wall one (which keeps counting while a phone sleeps).
 * Never by comparing with the server's clock.
 */
export function isStale(snapshot: { receivedAt: number; receivedWall: number }, now: Now, staleAfterMs: number): boolean {
  return now.mono - snapshot.receivedAt > staleAfterMs || now.wall - snapshot.receivedWall > staleAfterMs;
}

export function connection(list: ListState, now: Now, staleAfterMs: number): Connection {
  if (list.failure) return list.failure.kind;
  if (!list.last) return 'reading';
  return isStale(list.last, now, staleAfterMs) ? 'stale' : 'live';
}

/** Whether the selected session is on the current page's last good snapshot. */
export function listed(state: ViewState): boolean {
  const { detail, list } = state;
  if (!detail || !list.last) return true;
  return list.last.envelope.data.sessions.some((s) => s.id === detail.id);
}

const settle = <D>(o: Outcome<D>): Settled =>
  o.kind === 'ok'
    ? { ok: true, confirmed: true }
    : o.kind === 'busy'
      ? { ok: false, retryAfterMs: o.retryAfterMs, confirmed: serverDone(o) }
      : { ok: false, confirmed: serverDone(o) };

/** What the page shows while a read is late: it may still answer. */
const LATE: Failure = { kind: 'disconnected', cause: 'timeout', status: null };

export class Viewer {
  private readonly deps: ViewerDeps;
  private readonly timing: Timing;
  private state: ViewState = { list: { offset: 0, last: null, failure: null, previousMatched: null }, detail: null };
  private readonly listeners = new Set<() => void>();
  private list: Poller<Outcome<Inventory>> | null = null;
  private detail: Poller<Outcome<SessionView>> | null = null;
  private readonly listLane = new Lane();
  private readonly detailLane = new Lane();
  private running = false;

  constructor(deps: ViewerDeps, timing: Timing) {
    this.deps = deps;
    this.timing = timing;
  }

  getState(): ViewState {
    return this.state;
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  start(): void {
    if (this.running) return;
    this.running = true;
    this.startList();
    if (this.state.detail) this.startDetail(this.state.detail.id);
  }

  stop(): void {
    this.running = false;
    this.list?.stop();
    this.list = null;
    this.detail?.stop();
    this.detail = null;
  }

  /**
   * Another page of the core's order, by its `offset`. Its first read waits
   * for the read in flight to end. The open detail stays open.
   */
  goTo(offset: number): void {
    this.list?.retire();
    this.list = null;
    const list = this.state.list;
    const previousMatched = list.last?.envelope.coverage.counts?.matched ?? list.previousMatched;
    this.set({ ...this.state, list: { offset, last: null, failure: null, previousMatched } });
    if (this.running) this.startList();
  }

  /** Opens a session's detail (null closes it). Its first read waits for the detail read in flight to end. */
  select(id: string | null): void {
    this.detail?.retire();
    this.detail = null;
    if (id === null) {
      this.set({ ...this.state, detail: null });
      return;
    }
    const kept = this.state.detail?.id === id ? this.state.detail : { id, last: null, failure: null };
    this.set({ ...this.state, detail: kept });
    if (this.running) this.startDetail(id);
  }

  private set(next: ViewState): void {
    this.state = next;
    for (const listener of [...this.listeners]) listener();
  }

  private poller<D>(
    lane: Lane,
    intervalMs: number,
    url: string,
    parse: (json: unknown) => Envelope<D> | null,
    deliver: (o: Outcome<D>) => void,
    onTimeout: () => void,
  ) {
    return new Poller<Outcome<D>>({
      lane,
      intervalMs,
      timeoutMs: this.timing.requestTimeoutMs,
      holdMs: this.timing.laneHoldMs,
      onTimeout,
      backoffMs: this.timing.backoffMs,
      timers: this.deps.timers,
      visibility: this.deps.visibility,
      read: (signal) => read(this.deps.fetch, url, parse, signal),
      settle,
      deliver,
    });
  }

  private snapshot<D>(envelope: Envelope<D>): Snapshot<D> {
    return { envelope, receivedAt: this.deps.now(), receivedWall: this.deps.wall() };
  }

  private startList(): void {
    this.list?.retire();
    const offset = this.state.list.offset;
    const poller = this.poller(
      this.listLane,
      this.timing.listIntervalMs,
      inventoryUrl(offset),
      parseInventory,
      (o) => {
        if (this.list !== poller) return;
        const list = this.state.list;
        this.set({ ...this.state, list: o.kind === 'ok' ? { ...list, last: this.snapshot(o.envelope), failure: null } : { ...list, failure: o } });
      },
      () => {
        if (this.list === poller) this.set({ ...this.state, list: { ...this.state.list, failure: LATE } });
      },
    );
    this.list = poller;
    poller.start();
  }

  private startDetail(id: string): void {
    this.detail?.retire();
    const poller = this.poller(
      this.detailLane,
      this.timing.detailIntervalMs,
      sessionUrl(id),
      parseSessionView,
      (o) => {
        const detail = this.state.detail;
        if (this.detail !== poller || !detail || detail.id !== id) return;
        this.set({ ...this.state, detail: o.kind === 'ok' ? { id, last: this.snapshot(o.envelope), failure: null } : { ...detail, failure: o } });
      },
      () => {
        const detail = this.state.detail;
        if (this.detail === poller && detail && detail.id === id) this.set({ ...this.state, detail: { ...detail, failure: LATE } });
      },
    );
    this.detail = poller;
    poller.start();
  }
}
