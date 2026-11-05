// Synthetic `openwiz.inventory.v1` envelopes for the web proofs. Every value is
// invented: ids are `thr_web_NNNN` and `prj_web_NNNN`, titles say
// "Synthetic". They cover shapes the 0.0.1 fixtures don't: hostile text,
// many rows, every status tier, every unknown.

import { createHash } from 'node:crypto';

import type { Envelope, Gap, Inventory, Session, SessionView } from '../../src/envelope.ts';
import type { Delivery, Effects, Lookup, Mode, Preview, Report, Result } from '../../src/messaging.ts';

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

// ── Synthetic `send` shapes (contracts/messaging.v1.schema.json), as the
// CLI's --json, --preview --json and --check --json print them.
// Every value is invented; the words are the core's, copied from
// crates/core/src/messaging.


export const sha256 = (text: string): string => createHash('sha256').update(text, 'utf8').digest('hex');

const EFFECT_WORDS = {
  starts_turn: 'starts a turn; may wake it',
  queues_behind_turn: 'queues behind the current turn',
  asks_to_steer: 'asks to steer the current turn',
  waits_for_interaction: 'waits for the pending interaction to be answered',
  waits_for_host: 'waits for its host to come back online',
} as const;

export type ThreadKind = 'idle' | 'busy' | 'paused' | 'interaction' | 'unknown';

/** The effects the core works out for a thread of `kind`, sent in `mode`. */
export function effects(kind: ThreadKind, mode: Mode, readAt = T0, ageMs = 12): Effects {
  const notPaused = { known: { appears_paused: false, queued_messages: { known: 0 }, text: 'no paused queue seen' } };
  const known = (k: keyof typeof EFFECT_WORDS) => ({ known: { kind: k, text: EFFECT_WORDS[k] } });
  const base = { read_at: readAt, read_age_ms: ageMs, paused_queue: notPaused, needs_acknowledgement: false };
  switch (kind) {
    case 'idle':
      return { ...base, effect: known('starts_turn') };
    case 'busy':
      return { ...base, effect: known(mode === 'queue' ? 'queues_behind_turn' : 'asks_to_steer') };
    case 'interaction':
      return { ...base, effect: known('waits_for_interaction') };
    case 'paused':
      return {
        ...base,
        effect: known('starts_turn'),
        paused_queue: { known: { appears_paused: true, queued_messages: { known: 2 }, text: 'resumes the paused queue; 2 queued messages may run after this one' } },
        needs_acknowledgement: true,
      };
    case 'unknown':
      return { ...base, effect: { unknown: 'its status: the status syn-starting is not one OpenWiz knows' } };
  }
}

const requested = (mode: Mode) => (mode === 'queue' ? 'Queue requested' : 'Steer requested');
const target = (id: string) => ({ id, title: { known: `Synthetic web session ${Number(id.slice(-4))}` } });

export function preview(id: string, kind: ThreadKind | 'refused', mode: Mode = 'queue'): Preview {
  const head = { schema: 'openwiz.send-preview.v1' as const, target: target(id), requested_mode: mode, read_at: T0, gaps: [] };
  if (kind === 'refused') {
    const text = 'the target is archived, and archived targets are refused';
    return { ...head, status: 'refused', summary: `${requested(mode)} · preview: a send would be refused: ${text}; nothing was sent`, effects: null, refusal: { reason: 'target_archived', text } };
  }
  const e = effects(kind, mode);
  if (kind === 'paused') {
    return {
      ...head,
      status: 'needs_acknowledgement',
      summary: `${requested(mode)} · preview: resumes the paused queue; 2 queued messages may run after this one; a send needs the owner's acknowledgement to resume it; nothing was sent`,
      effects: e,
      refusal: {
        reason: 'paused_queue_not_acknowledged',
        text: "the target's queue appears paused, or that cannot be ruled out, and sending would resume it; nothing was sent without the owner's acknowledgement",
      },
    };
  }
  const words = 'known' in e.effect ? `a send ${e.effect.known.text}` : `the effect is unknown (${e.effect.unknown})`;
  return { ...head, status: 'ready', summary: `${requested(mode)} · preview: ${words}; nothing was sent`, effects: e, refusal: null };
}

const UNCERTAIN_TAIL = 'The message may or may not have been accepted; the text is kept and nothing is resent';

export const DELIVERIES = {
  sent: {
    kind: 'accepted',
    joined_running_turn: { unknown: 'room-cli 0.43.1 does not report whether a message joined the running turn' },
    text: 'sent: the provider accepted the message, not that the work completed',
  },
  queuedInteraction: { kind: 'queued', waiting_on: 'interaction', queued_message_id: 'syn-queued-0001', text: 'queued: waiting on an interaction' },
  queuedBusy: { kind: 'queued', waiting_on: 'thread_busy', queued_message_id: 'syn-queued-0002', text: 'queued: thread busy' },
  failed: { kind: 'failed', reason: 'invalid_input', text: 'failed: the provider rejected the input' },
  unsupported: {
    kind: 'failed',
    reason: 'unsupported_mode',
    text: 'failed: unsupported mode: the provider does not support the requested mode for this thread; no other mode was tried',
  },
  uncertain: {
    kind: 'uncertain',
    reason: 'timed_out',
    text: `uncertain: the provider did not answer in time. ${UNCERTAIN_TAIL}`,
    reconciliation: { kind: 'not_read' },
    attribution: 'none: room-cli 0.43.1 documents no identifier that ties a delivered message to this request',
  },
  /** Synthetic: one message matching on text and time was observed; that is not attribution. */
  uncertainMatched: {
    kind: 'uncertain',
    reason: 'timed_out',
    text: `uncertain: the provider did not answer in time. ${UNCERTAIN_TAIL}`,
    reconciliation: { kind: 'observed', matching: 1, total: 3 },
    attribution: 'none: room-cli 0.43.1 documents no identifier that ties a delivered message to this request',
  },
} as const satisfies Record<string, Delivery>;

const STATUS: Record<Delivery['kind'], Report['status']> = { queued: 'queued', accepted: 'accepted', failed: 'failed', uncertain: 'uncertain' };

/** The CLI's report for one send of `text` with `opId`. */
export function report(id: string, opId: string, mode: Mode, text: string, result: Result, readAt = T0): Report {
  const status: Report['status'] = result.kind === 'outcome' ? STATUS[result.delivery.kind] : result.kind;
  const tail =
    result.kind === 'outcome'
      ? result.delivery.text
      : result.kind === 'in_progress'
        ? `in progress: another process is sending op ${opId}; no call was made here`
        : result.kind === 'conflicting_reuse'
          ? `refused: op ${opId} is bound to a different ${result.differs.join(', ')}; no call was made`
          : `refused: an earlier send of this text to this target is uncertain (op ${result.earlier.map((e) => e.op_id).join(', op ')}), and this one may duplicate it; nothing was sent`;
  return {
    schema: 'openwiz.messaging.v1',
    op_id: opId,
    target: target(id),
    source: { name: 'room-cli', version: { known: '0.43.1' }, tested_version: '0.43.1', version_tested: true },
    requested_mode: mode,
    status,
    summary: `${requested(mode)} · ${tail}`,
    result,
    effects: result.kind === 'outcome' ? effects('idle', mode, readAt) : null,
    text_sha256: sha256(text),
    text_bytes: Buffer.byteLength(text, 'utf8'),
    gaps: [],
    claimed_at: readAt,
    dispatching_at: result.kind === 'outcome' && result.delivery.kind !== 'failed' ? readAt + 5 : null,
    finished_at: result.kind === 'in_progress' ? null : readAt + 10,
  };
}

export const outcome = (delivery: Delivery): Result => ({ kind: 'outcome', delivery, replayed: false, recovered: false });

export function lookup(id: string, opId: string, recorded: Report | null): Lookup {
  return recorded
    ? { schema: 'openwiz.send-lookup.v1', op_id: opId, target: id, summary: recorded.summary, result: { kind: 'recorded', report: { ...recorded, result: recorded.result.kind === 'outcome' ? { ...recorded.result, replayed: true } : recorded.result } } }
    : { schema: 'openwiz.send-lookup.v1', op_id: opId, target: id, summary: 'no record: no send with this op id was claimed; nothing was sent', result: { kind: 'no_record' } };
}
