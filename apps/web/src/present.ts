// Presentation of envelope values, as text. Nothing here decides membership,
// order, visibility, truncation or coverage: those come from the envelope.

import type { Counts, Fact, Grouping, Session } from './envelope.ts';

export type PageGroup = { key: string; group: Grouping; sessions: Session[] };

/**
 * One page's rows under their group headings: groups in the order their first
 * row appears, rows in the page's order (the same as `openwiz ls`).
 */
export function groupPage(sessions: readonly Session[]): PageGroup[] {
  const groups = new Map<string, PageGroup>();
  for (const session of sessions) {
    const key = session.group.kind === 'project' ? `project:${session.group.id}` : 'ungrouped';
    const group = groups.get(key);
    if (group) group.sessions.push(session);
    else groups.set(key, { key, group: session.group, sessions: [session] });
  }
  return [...groups.values()];
}

/** `unknown (reason)`, word for word, as the CLI prints it. */
export function unknownText(reason: string): string {
  return `unknown (${reason})`;
}

export function factText(fact: Fact<string>): string {
  return 'known' in fact ? fact.known : unknownText(fact.unknown);
}

/** A cut text, with `…` right after what was kept, as the CLI prints it. */
export function boundedText(text: string, truncated: boolean): string {
  return truncated ? `${text}…` : text;
}

/** The CLI's age: whole days, hours, minutes or seconds. */
export function ageText(ms: number): string {
  const sign = ms < 0 ? '-' : '';
  const s = Math.trunc(Math.abs(ms) / 1000);
  for (const [unit, size] of [
    ['d', 86_400],
    ['h', 3_600],
    ['m', 60],
  ] as const) {
    if (s >= size) return `${sign}${Math.trunc(s / size)}${unit}`;
  }
  return `${sign}${s}s`;
}

/** Device-local `HH:MM:SS`, with the date when it is not `now`'s day. */
export function clockText(epochMs: number, now: Date): string {
  const d = new Date(epochMs);
  const two = (n: number) => String(n).padStart(2, '0');
  const time = `${two(d.getHours())}:${two(d.getMinutes())}:${two(d.getSeconds())}`;
  return d.toDateString() === now.toDateString() ? time : `${d.getFullYear()}-${two(d.getMonth() + 1)}-${two(d.getDate())} ${time}`;
}

export type PageRange = {
  first: number;
  last: number;
  matched: number;
  previousOffset: number | null;
  nextOffset: number | null;
};

/** The page's place, from the envelope's own counts. */
export function pageRange(counts: Counts): PageRange {
  const { offset, limit, shown, matched } = counts;
  return {
    first: shown > 0 ? offset + 1 : 0,
    last: shown > 0 ? offset + shown : 0,
    matched,
    previousOffset: offset > 0 ? Math.max(0, offset - Math.max(limit, 1)) : null,
    nextOffset: limit > 0 && shown > 0 && offset + shown < matched ? offset + limit : null,
  };
}
