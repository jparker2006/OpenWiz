// Test doubles for the unit proofs: a manual clock, a page visibility switch
// and a scripted fetch that counts requests in flight.

import type { FetchLike, ResponseLike } from '../../src/api.ts';
import type { Timers, Visibility } from '../../src/poller.ts';

type Timer = { at: number; seq: number; fn: () => void };

/** A clock that only moves when `advance` is called. */
export class FakeClock implements Timers {
  now = 0;
  private seq = 0;
  private timers = new Map<number, Timer>();

  setTimeout(fn: () => void, ms: number): unknown {
    const id = ++this.seq;
    this.timers.set(id, { at: this.now + Math.max(0, ms), seq: id, fn });
    return id;
  }

  clearTimeout(handle: unknown): void {
    this.timers.delete(handle as number);
  }

  pending(): number {
    return this.timers.size;
  }

  /** Moves time forward, firing due timers in order and letting promises settle after each. */
  async advance(ms: number): Promise<void> {
    const end = this.now + ms;
    for (;;) {
      await settle();
      let next: [number, Timer] | undefined;
      for (const entry of this.timers) {
        if (entry[1].at <= end && (!next || entry[1].at < next[1].at || (entry[1].at === next[1].at && entry[1].seq < next[1].seq))) {
          next = entry;
        }
      }
      if (!next) break;
      this.timers.delete(next[0]);
      this.now = next[1].at;
      next[1].fn();
    }
    this.now = end;
    await settle();
  }
}

/** Lets queued promise callbacks run. */
export async function settle(): Promise<void> {
  for (let i = 0; i < 20; i++) await Promise.resolve();
}

export class FakeVisibility implements Visibility {
  private isHidden = false;
  private listeners = new Set<() => void>();

  hidden(): boolean {
    return this.isHidden;
  }

  subscribe(onChange: () => void): () => void {
    this.listeners.add(onChange);
    return () => this.listeners.delete(onChange);
  }

  set(hidden: boolean): void {
    this.isHidden = hidden;
    for (const listener of [...this.listeners]) listener();
  }
}

export type FakeReply = {
  status: number;
  type?: string;
  headers?: Record<string, string>;
  body?: string;
};

type Pending = { url: string; resolve: (r: ResponseLike) => void; reject: (e: unknown) => void; done: boolean };

/**
 * A fetch whose replies the test gives by hand (`reply`, `fail`), or at once
 * from `auto`. It counts requests in flight per kind (list or detail).
 */
export class FakeFetch {
  readonly urls: string[] = [];
  readonly inits: RequestInit[] = [];
  readonly pending: Pending[] = [];
  inFlight = { list: 0, detail: 0 };
  maxInFlight = { list: 0, detail: 0 };
  auto: ((url: string) => FakeReply) | null = null;

  readonly fetch: FetchLike = (url, init) => {
    this.urls.push(url);
    this.inits.push(init);
    const kind = url.startsWith('/api/v1/sessions/') ? 'detail' : 'list';
    this.inFlight[kind]++;
    this.maxInFlight[kind] = Math.max(this.maxInFlight[kind], this.inFlight[kind]);
    return new Promise<ResponseLike>((resolve, reject) => {
      const entry: Pending = {
        url,
        resolve: (r) => {
          if (entry.done) return;
          entry.done = true;
          this.inFlight[kind]--;
          resolve(r);
        },
        reject: (e) => {
          if (entry.done) return;
          entry.done = true;
          this.inFlight[kind]--;
          reject(e);
        },
        done: false,
      };
      this.pending.push(entry);
      init.signal?.addEventListener('abort', () => entry.reject(new DOMException('aborted', 'AbortError')));
      if (this.auto) entry.resolve(response(this.auto(url)));
    });
  };

  /** Answers the oldest unanswered request. */
  reply(r: FakeReply): void {
    const entry = this.pending.find((p) => !p.done);
    if (!entry) throw new Error('no request to answer');
    entry.resolve(response(r));
  }

  /** Answers the oldest unanswered request for exactly `url`. */
  replyTo(url: string, r: FakeReply): void {
    const entry = this.pending.find((p) => !p.done && p.url === url);
    if (!entry) throw new Error(`no request for ${url} to answer`);
    entry.resolve(response(r));
  }

  /** Fails the oldest unanswered request as a network error. */
  fail(): void {
    const entry = this.pending.find((p) => !p.done);
    if (!entry) throw new Error('no request to fail');
    entry.reject(new TypeError('Failed to fetch'));
  }

  open(): Pending[] {
    return this.pending.filter((p) => !p.done);
  }
}

export function response(r: FakeReply): ResponseLike {
  const headers = new Map(Object.entries(r.headers ?? {}).map(([k, v]) => [k.toLowerCase(), v]));
  return {
    type: r.type ?? 'basic',
    status: r.status,
    headers: { get: (name: string) => headers.get(name.toLowerCase()) ?? null },
    text: async () => r.body ?? '',
  };
}

export const json = (status: number, value: unknown, extra: Record<string, string> = {}): FakeReply => ({
  status,
  headers: { 'Content-Type': 'application/json', ...extra },
  body: JSON.stringify(value),
});
