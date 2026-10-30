// Synthetic `openwiz.inventory.v1` envelopes for the web proofs. Every value is
// invented: ids are `thr_web_NNNN` and `prj_web_NNNN`, titles say
// "Synthetic". They cover shapes the 0.0.1 fixtures don't: hostile text,
// many rows, every status tier, every unknown.

import type { Envelope, Gap, Inventory, Session, SessionView } from '../../src/envelope.ts';

/** A fixed synthetic snapshot time: 2027-01-14T08:00:00Z. */
export const T0 = 1_799_913_600_000;

export const pad = (n: number): string => String(n).padStart(4, '0');

export function session(n: number, over: Partial<Session> = {}): Session {
  return {
    id: `thr_web_${pad(n)}`,
    title: { known: `Synthetic web session ${n}` },
    provider: { known: 'codex' },
    status: { known: 'idle' },
    location: { known: { kind: 'cloud' } },
    group: { kind: 'project', id: 'prj_web_0001', name: { known: 'Synthetic web project one' } },
    parent: { known: null },
    hidden: { known: false },
    archived: { known: false },
    recap: { known: { kind: 'purpose_as_reported', text: `Synthetic purpose ${n}.`, truncated: false } },
    reported_configuration: { model: { unknown: 'not read: ls without -l' }, effort: { unknown: 'not read: ls without -l' } },
    updated_at: { known: T0 - n * 60_000 },
    age_ms: { known: n * 60_000 },
    ...over,
  };
}

export type InventoryOptions = {
  state?: 'complete' | 'partial';
  gaps?: Gap[];
  offset?: number;
  limit?: number;
  matched?: number;
  snapshotAt?: number;
  countsNull?: boolean;
};

export function inventory(sessions: Session[], o: InventoryOptions = {}): Envelope<Inventory> {
  const offset = o.offset ?? 0;
  const limit = o.limit ?? 100;
  const matched = o.matched ?? offset + sessions.length;
  return {
    schema: 'openwiz.inventory.v1',
    source: { name: 'room-cli', version: { known: '0.43.1' }, tested_version: '0.43.1', version_tested: true },
    coverage: {
      state: o.state ?? 'complete',
      counts: o.countsNull
        ? null
        : {
            listed: matched,
            unreadable: 0,
            excluded_hidden: 0,
            excluded_archived: 0,
            hidden_read: false,
            archived_read: false,
            matched,
            offset,
            limit,
            shown: sessions.length,
          },
    },
    freshness: { snapshot_at: o.snapshotAt ?? T0 },
    gaps: o.gaps ?? [],
    data: { sessions },
  };
}

export type ViewOptions = {
  state?: 'complete' | 'partial';
  gaps?: Gap[];
  finalOutput?: SessionView['final_output'];
  reportedError?: SessionView['reported_error'];
  snapshotAt?: number;
};

export function sessionView(s: Session, o: ViewOptions = {}): Envelope<SessionView> {
  return {
    schema: 'openwiz.inventory.v1',
    source: { name: 'room-cli', version: { known: '0.43.1' }, tested_version: '0.43.1', version_tested: true },
    coverage: { state: o.state ?? 'complete', counts: null },
    freshness: { snapshot_at: o.snapshotAt ?? T0 },
    gaps: o.gaps ?? [],
    data: {
      session: s,
      final_output: o.finalOutput ?? { known: { kind: 'reported', text: `Synthetic final output for ${s.id}.`, truncated: false } },
      reported_error: o.reportedError ?? { known: null },
    },
  };
}

/** Many rows in a few groups, for scrolling and paging. */
export function manyRows(count: number, first = 1): Session[] {
  return Array.from({ length: count }, (_, i) => {
    const n = first + i;
    const g = (n % 3) + 1;
    return session(n, {
      group: { kind: 'project', id: `prj_web_000${g}`, name: { known: `Synthetic web project ${g}` } },
      status: { known: ['idle', 'active', 'idle', 'error'][n % 4] ?? 'idle' },
    });
  });
}

// Text that must stay inert (V3). The marker `__pwned` is set by any payload
// that runs.
export const HOSTILE = {
  html: '<img src=x onerror="window.__pwned=1"><b>bold</b>',
  script: '<script>window.__pwned=1</script>',
  link: '<a href="javascript:window.__pwned=1">click</a>',
  markdown: '**bold** _it_ [link](javascript:window.__pwned=1) `code` # heading',
  bidi: 'Synthetic ‮evil‬ name ⁦isolate⁩ end',
  entity: '&lt;b&gt; &amp;amp; &#x3C;i&#x3E;',
  long: `Synthetic ${'W'.repeat(190)}…`,
  newlines: 'Synthetic line one\nline two\r\nline three four',
};

const hostileWords = Object.values(HOSTILE);

/** One row per hostile string, with that string in every text field it can reach. */
export function hostileSessions(): Session[] {
  return hostileWords.map((text, i) =>
    session(9000 + i, {
      title: { known: text },
      provider: { known: text },
      status: { known: text },
      location: { known: { kind: 'local', machine: { known: text } } },
      group:
        i % 2 === 0
          ? { kind: 'project', id: `prj_web_90${pad(i)}`, name: { known: text } }
          : { kind: 'ungrouped', reason: text },
      recap: { known: { kind: 'purpose_as_reported', text: text.slice(0, 120), truncated: text.length > 120 } },
      reported_configuration: { model: { known: text }, effort: { unknown: text } },
    }),
  );
}

export function hostileInventory(): Envelope<Inventory> {
  return inventory(hostileSessions(), {
    state: 'partial',
    gaps: hostileWords.map((text, i) => ({ kind: 'command_failed', subject: i % 2 ? null : text, detail: text })),
  });
}

export function hostileView(s: Session): Envelope<SessionView> {
  const text = typeof s.title === 'object' && 'known' in s.title ? s.title.known : 'Synthetic';
  return sessionView(s, {
    state: 'partial',
    gaps: [{ kind: 'command_failed', subject: s.id, detail: text }],
    finalOutput: { known: { kind: 'reported', text: `${text}\n${HOSTILE.html}\n${HOSTILE.markdown}\n${'X'.repeat(3600)}`, truncated: true } },
    reportedError: { known: { text, truncated: false } },
  });
}

/** One row for each status shape (V2): t1, t3, t4, t5 by word, and an unknown. */
export function statusSessions(): Session[] {
  return [
    session(1, { status: { known: 'error' } }),
    session(2, { status: { known: 'active' } }),
    session(3, { status: { known: 'idle' } }),
    session(4, { status: { known: 'syn-other' } }),
    session(5, { status: { unknown: 'missing field status' } }),
  ];
}
