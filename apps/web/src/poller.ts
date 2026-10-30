// One refresh loop: at most one read in flight. The next read starts only
// after the last one ended or timed out (V4), after the interval, or after a
// capped backoff when it failed. Paused while the page is hidden; one read
// when it returns (V7).

export interface Timers {
  setTimeout(fn: () => void, ms: number): unknown;
  clearTimeout(handle: unknown): void;
}

export interface Visibility {
  hidden(): boolean;
  subscribe(onChange: () => void): () => void;
}

/**
 * How a finished read schedules the next one. `confirmed`: the server is known
 * to be done with the read (it answered), so the next read may start.
 */
export type Settled = { ok: boolean; retryAfterMs?: number | null; confirmed: boolean };

/**
 * One read at a time across loops. A loop that replaces another (another
 * page, another session) waits for the other's read to end or time out: the
 * server keeps running a read whose page went away, so aborting it would not
 * stop it (V4). Only the latest waiter is kept.
 */
export class Lane {
  private busy = false;
  private waiting: (() => void) | null = null;

  enter(start: () => void): void {
    if (this.busy) {
      this.waiting = start;
      return;
    }
    this.busy = true;
    start();
  }

  leave(): void {
    this.busy = false;
    const next = this.waiting;
    this.waiting = null;
    if (next) this.enter(next);
  }

  forget(start: () => void): void {
    if (this.waiting === start) this.waiting = null;
  }
}

export type PollerOptions<R> = {
  /** Shared with the loops that may replace this one. */
  lane?: Lane;
  intervalMs: number;
  /** When the page is told a read is late (`onTimeout`). The read goes on. */
  timeoutMs: number;
  /** From its start, how long a read the server has not answered keeps the lane; then it is given up. */
  holdMs: number;
  backoffMs: readonly number[];
  onTimeout?: () => void;
  timers: Timers;
  visibility: Visibility;
  /** Resolves with the result, or `'stopped'` after `stop()`. Never rejects. */
  read(signal: AbortSignal): Promise<R | 'stopped'>;
  settle(result: R): Settled;
  deliver(result: R): void;
};

export class Poller<R> {
  private readonly o: PollerOptions<R>;
  private readonly lane: Lane;
  private controller: AbortController | null = null;
  private queued = false;
  private timer: unknown = null;
  private timeout: unknown = null;
  /** The server's bound for the current read, from its start. */
  private hold: unknown = null;
  private holdOver = false;
  /** The read ended without the server's answer: the lane stays held until `hold`. */
  private holding = false;
  private failures = 0;
  private running = false;
  private hidden = false;
  private unsubscribe: (() => void) | null = null;

  constructor(options: PollerOptions<R>) {
    this.o = options;
    this.lane = options.lane ?? new Lane();
  }

  get inFlight(): boolean {
    return this.controller !== null;
  }

  /** Starts the loop: a read now (or as soon as the page is visible). */
  start(): void {
    if (this.running) return;
    this.running = true;
    this.hidden = this.o.visibility.hidden();
    this.unsubscribe = this.o.visibility.subscribe(() => this.visibilityChanged());
    this.run();
  }

  /** Aborts the read in flight; nothing more is read or delivered. For the page going away. */
  stop(): void {
    this.retire();
    if (this.holding) this.releaseLane();
    this.controller?.abort('stop');
  }

  /**
   * Nothing more is read or delivered, but the read in flight runs on until it
   * ends or times out, and holds the lane until then.
   */
  retire(): void {
    this.running = false;
    this.unsubscribe?.();
    this.unsubscribe = null;
    this.clearTimer();
    this.clearTimeout();
    this.lane.forget(this.begin);
    this.queued = false;
  }

  private visibilityChanged(): void {
    if (!this.running) return;
    const hidden = this.o.visibility.hidden();
    if (hidden === this.hidden) return;
    this.hidden = hidden;
    if (hidden) {
      // Paused: no timer runs while hidden. A read in flight may finish.
      this.clearTimer();
      return;
    }
    // Back: one read now, unless one is already in flight.
    this.run();
  }

  private run(): void {
    if (!this.running || this.controller !== null || this.queued || this.hidden) return;
    this.clearTimer();
    this.queued = true;
    this.lane.enter(this.begin);
  }

  /** Starts the read, once the lane is free. */
  private readonly begin = (): void => {
    this.queued = false;
    if (!this.running || this.hidden) {
      this.lane.leave();
      return;
    }
    const controller = new AbortController();
    this.controller = controller;
    this.holdOver = false;
    this.timeout = this.o.timers.setTimeout(() => {
      this.timeout = null;
      if (this.running) this.o.onTimeout?.();
    }, this.o.timeoutMs);
    this.hold = this.o.timers.setTimeout(() => this.holdEnded(controller), this.o.holdMs);
    void this.o.read(controller.signal).then(
      (result) => this.finished(controller, result),
      () => this.finished(controller, 'stopped'),
    );
  };

  /** The server's bound for the read is over: give the read up, or end the lane's hold. */
  private holdEnded(controller: AbortController): void {
    this.hold = null;
    this.holdOver = true;
    if (this.controller === controller) controller.abort('timeout');
    else if (this.holding) this.releaseLane();
  }

  private releaseLane(): void {
    this.holding = false;
    if (this.hold !== null) this.o.timers.clearTimeout(this.hold);
    this.hold = null;
    this.lane.leave();
  }

  private finished(controller: AbortController, result: R | 'stopped'): void {
    if (this.controller !== controller) return;
    this.controller = null;
    this.clearTimeout();
    if (result === 'stopped') {
      this.releaseLane();
      return;
    }
    let settled: Settled = { ok: false, confirmed: false };
    try {
      settled = this.o.settle(result);
      if (this.running) this.o.deliver(result);
    } finally {
      // The next read may start once the server is known to be done with this
      // one: it answered, or its bound has passed since the read was sent.
      if (settled.confirmed || this.holdOver) this.releaseLane();
      else this.holding = true;
      this.failures = settled.ok ? 0 : this.failures + 1;
      const backoff = this.o.backoffMs[Math.min(this.failures, this.o.backoffMs.length) - 1] ?? this.o.intervalMs;
      // A server's retry hint is honoured, but never past the backoff's cap.
      const cap = this.o.backoffMs.at(-1) ?? this.o.intervalMs;
      const delay = settled.ok ? this.o.intervalMs : Math.max(backoff, Math.min(settled.retryAfterMs ?? 0, cap));
      if (this.running && !this.hidden) this.timer = this.o.timers.setTimeout(() => this.next(), delay);
    }
  }

  private next(): void {
    this.timer = null;
    this.run();
  }

  private clearTimer(): void {
    if (this.timer !== null) this.o.timers.clearTimeout(this.timer);
    this.timer = null;
  }

  private clearTimeout(): void {
    if (this.timeout !== null) this.o.timers.clearTimeout(this.timeout);
    this.timeout = null;
  }
}
