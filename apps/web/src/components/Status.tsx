// Status as a word and a shape: the session marker (V2), the connection pill
// and the notice that says what went wrong (V6). Session text is plain text,
// isolated for bidirectional text (V3).

import type { Failure } from '../api.ts';
import type { FactString } from '../envelope.ts';
import { statusMarker } from '../markers.ts';
import { clockText } from '../present.ts';
import type { Connection } from '../viewer.ts';

export function Marker({ status }: { status: FactString }) {
  const m = statusMarker(status);
  return (
    <span className={`mk ${m.tier}`} data-part="status" data-tier={m.tier}>
      <span className="glyph" aria-hidden="true">
        {m.glyph}
      </span>{' '}
      <bdi className="word">{m.word}</bdi>
    </span>
  );
}

const CONNECTION_WORDS: Record<Connection, string> = {
  reading: 'reading',
  live: 'live',
  stale: 'stale',
  disconnected: 'disconnected',
  'signed-out': 'signed out',
  busy: 'busy',
  error: 'error',
};

export function ConnectionPill({ connection }: { connection: Connection }) {
  return (
    <p className="conn" data-connection={connection} role="status">
      <span className="shape" aria-hidden="true" />
      <span className="word">{CONNECTION_WORDS[connection]}</span>
    </p>
  );
}

function why(f: Failure): string {
  switch (f.kind) {
    case 'signed-out':
      return 'your sign-in has expired';
    case 'disconnected':
      if (f.cause === 'timeout') return 'can’t reach OpenWiz on your Mac: no answer in time';
      if (f.cause === 'gateway') return `can’t reach OpenWiz on your Mac: HTTP ${f.status ?? ''} from the network`;
      return 'can’t reach OpenWiz on your Mac';
    case 'busy':
      return 'the server is busy; retrying';
    case 'error':
      return `unexpected response (HTTP ${f.status})`;
  }
}

const TITLES: Record<Failure['kind'], string> = {
  'signed-out': 'Signed out',
  disconnected: 'Disconnected',
  busy: 'Busy',
  error: 'Error',
};

/** What went wrong, in the server's or the core's own words when it gave any, and the last good snapshot. */
export function Notice({ failure, lastGoodAt, now }: { failure: Failure; lastGoodAt: number | null; now: Date }) {
  const words = (failure.kind === 'error' || failure.kind === 'busy') && failure.error ? failure.error : null;
  return (
    <div className="notice" data-notice={failure.kind}>
      <p className="notice-head">
        <span className="shape" aria-hidden="true" /> <b>{TITLES[failure.kind]}</b>
      </p>
      <p className="notice-body">
        {words ? (
          <>
            <code>{words.kind}</code>: <bdi>{words.message}</bdi>
          </>
        ) : (
          why(failure)
        )}
      </p>
      <p className="meta">{lastGoodAt === null ? 'no snapshot yet' : `last good snapshot ${clockText(lastGoodAt, now)}`}</p>
      {failure.kind === 'signed-out' ? (
        <a className="btn" href="/">
          Sign in again
        </a>
      ) : null}
    </div>
  );
}
