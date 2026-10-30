// The list: one page of the core's order under its group headings, thin
// rows (name, status, age), the empty states and the page controls. Refreshes
// keep the rows on screen where they were (V5).

import { Component } from 'react';
import type { ReactNode } from 'react';

import type { Envelope, Inventory as InventoryData, Session } from '../envelope.ts';
import { ageText, clockText, factText, groupPage, pageRange, unknownText } from '../present.ts';
import { Marker } from './Status.tsx';

function Row({ session, selected, onSelect }: { session: Session; selected: boolean; onSelect: (id: string) => void }) {
  const known = 'known' in session.title;
  return (
    <li>
      <button
        type="button"
        className={`row${selected ? ' on' : ''}`}
        data-session-id={session.id}
        aria-current={selected ? 'true' : undefined}
        onClick={() => {
          performance.mark('detail-tap');
          onSelect(session.id);
        }}
      >
        <span className="row-top">
          <span className={`title${known ? '' : ' unknown'}`} data-part="title">
            <bdi>{factText(session.title)}</bdi>
          </span>
          <span className="age" data-part="age">
            {'known' in session.age_ms ? ageText(session.age_ms.known) : unknownText(session.age_ms.unknown)}
          </span>
        </span>
        <span className="row-meta">
          <Marker status={session.status} />
          {session.group.kind === 'ungrouped' ? (
            <span className="reason" data-part="reason">
              <bdi>{session.group.reason}</bdi>
            </span>
          ) : null}
        </span>
      </button>
    </li>
  );
}

type Anchor = { id: string; top: number };

const rowsOnScreen = (): Anchor[] => {
  const out: Anchor[] = [];
  for (const el of document.querySelectorAll<HTMLElement>('main [data-session-id]')) {
    const r = el.getBoundingClientRect();
    if (r.bottom > 48 && r.top < window.innerHeight && el.dataset['sessionId']) out.push({ id: el.dataset['sessionId'], top: r.top });
    if (out.length >= 8) break;
  }
  return out;
};

const rowById = (id: string): HTMLElement | null => {
  for (const el of document.querySelectorAll<HTMLElement>('main [data-session-id]')) if (el.dataset['sessionId'] === id) return el;
  return null;
};

/**
 * Keeps the rows on screen where they were when a refresh lands (new rows, or
 * a failure notice above them): the first of them still listed stays at the
 * same height. At the very top of the page, new rows simply appear.
 */
export class KeepPlace extends Component<{ snapshot: unknown; children: ReactNode }> {
  override getSnapshotBeforeUpdate(prev: { snapshot: unknown }): Anchor[] | null {
    if (prev.snapshot === this.props.snapshot || window.scrollY <= 0) return null;
    return rowsOnScreen();
  }

  override componentDidUpdate(_prev: unknown, _state: unknown, anchors: Anchor[] | null): void {
    if (!anchors) return;
    for (const a of anchors) {
      const el = rowById(a.id);
      if (!el) continue;
      const moved = el.getBoundingClientRect().top - a.top;
      if (moved !== 0) window.scrollBy(0, moved);
      return;
    }
  }

  override render() {
    return this.props.children;
  }
}

export type EmptyKind = 'verified-empty' | 'empty-page' | 'empty-partial' | 'empty-stale';

export function Inventory({
  envelope,
  previousMatched,
  selectedId,
  fresh,
  now,
  onSelect,
  onPage,
}: {
  envelope: Envelope<InventoryData>;
  previousMatched: number | null;
  selectedId: string | null;
  fresh: boolean;
  now: Date;
  onSelect: (id: string) => void;
  onPage: (offset: number) => void;
}) {
  const sessions = envelope.data.sessions;
  const counts = envelope.coverage.counts;
  const range = counts ? pageRange(counts) : null;
  let empty: EmptyKind | null = null;
  if (sessions.length === 0) {
    if (envelope.coverage.state === 'partial') empty = 'empty-partial';
    else if (counts && counts.matched > 0) empty = 'empty-page';
    else empty = fresh ? 'verified-empty' : 'empty-stale';
  }
  const changed = previousMatched !== null && counts !== null && counts.matched !== previousMatched;
  return (
    <>
      {changed ? (
        <p className="tag dashed page-change" data-part="page-change">
          The list changed between pages: {previousMatched} → {counts.matched} sessions. A session may have moved across pages, unseen or shown
          twice.
        </p>
      ) : null}
      {groupPage(sessions).map((g) => (
          <section className="group" key={g.key} data-group-key={g.key}>
            <h2 className="kicker">{g.group.kind === 'project' ? <bdi>{factText(g.group.name)}</bdi> : 'ungrouped'}</h2>
            <ul className="rows">
              {g.sessions.map((s) => (
                <Row key={s.id} session={s} selected={s.id === selectedId} onSelect={onSelect} />
              ))}
            </ul>
          </section>
        ))}
      {empty ? (
        <div className="empty" data-part="empty" data-kind={empty}>
          <p className="body">
            {empty === 'verified-empty'
              ? 'No sessions'
              : empty === 'empty-page'
                ? 'No sessions on this page'
                : empty === 'empty-partial'
                  ? 'No sessions read: the snapshot is partial'
                  : `No sessions as of ${clockText(envelope.freshness.snapshot_at, now)}`}
          </p>
          {empty === 'empty-page' ? (
            <button type="button" className="btn" onClick={() => onPage(0)}>
              First page
            </button>
          ) : null}
        </div>
      ) : null}
      {range && (range.previousOffset !== null || range.nextOffset !== null) ? (
        <nav className="paging" data-part="paging" aria-label="Pages">
          <button type="button" className="btn" data-part="previous" disabled={range.previousOffset === null} onClick={() => range.previousOffset !== null && onPage(range.previousOffset)}>
            Previous
          </button>
          <span className="meta" data-part="range">
            {range.first}–{range.last} of {range.matched}
          </span>
          <button type="button" className="btn" data-part="next" disabled={range.nextOffset === null} onClick={() => range.nextOffset !== null && onPage(range.nextOffset)}>
            Next
          </button>
        </nav>
      ) : null}
      {range && (range.previousOffset !== null || range.nextOffset !== null) ? (
        <p className="meta" data-part="paging-note">
          Each page is a separate snapshot: a session that changes between reads can move to another page, be missed or show twice.
        </p>
      ) : null}
    </>
  );
}
