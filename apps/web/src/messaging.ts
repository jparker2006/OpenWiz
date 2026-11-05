// The `send` shapes the composer reads, as `contracts/messaging.v1.schema.json`
// defines them: the report (`openwiz send --json`), the preview (`--preview`)
// and the lookup (`--check`). One type per schema definition it reads, named
// after it in PascalCase, with exactly the schema's property names: `npm run
// check` compares them. The web computes nothing from them but words.

import { bool, count, exact, exactWith, fact, int, is, isNull, list, oneOf, str } from './envelope.ts';
import type { Check, Fact, FactString } from './envelope.ts';

export const REPORT_SCHEMA_ID = 'openwiz.messaging.v1';
export const PREVIEW_SCHEMA_ID = 'openwiz.send-preview.v1';
export const LOOKUP_SCHEMA_ID = 'openwiz.send-lookup.v1';

export type Mode = 'queue' | 'steer';

export const FAIL_REASONS = [
  'not_found',
  'invalid_input',
  'unsupported_mode',
  'interrupted_before_dispatch',
  'untested_version',
  'target_not_read',
  'target_archived',
  'target_hidden',
  'target_visibility_unknown',
  'paused_queue_not_acknowledged',
  'record_not_written',
  'budget_exhausted',
  'call_not_started',
] as const;
export type FailReason = (typeof FAIL_REASONS)[number];

export type RecordState = 'claimed' | 'dispatching' | 'finished';

export type Target = { id: string; title: FactString };

export type Source = { name: string; version: FactString; tested_version: string; version_tested: boolean };

export type Gap = { subject: string; detail: string };

export type Effect = {
  kind: 'starts_turn' | 'queues_behind_turn' | 'asks_to_steer' | 'waits_for_interaction' | 'waits_for_host';
  text: string;
};

export type PausedQueue = { appears_paused: boolean; queued_messages: Fact<number>; text: string };

export type Effects = {
  read_at: number;
  read_age_ms: number;
  effect: Fact<Effect>;
  paused_queue: Fact<PausedQueue>;
  needs_acknowledgement: boolean;
};

export type Reconciliation = { kind: 'not_read' } | { kind: 'observed'; matching: number; total: number } | { kind: 'attributed' };

export type Delivery =
  | { kind: 'queued'; waiting_on: string; queued_message_id: string; text: string }
  | { kind: 'accepted'; joined_running_turn: Fact<boolean>; text: string }
  | { kind: 'failed'; reason: FailReason; text: string }
  | { kind: 'uncertain'; reason: string; text: string; reconciliation: Reconciliation; attribution: string };

export type EarlierSend = { op_id: string; mode: Mode; claimed_at: number; in_flight: boolean; reconciliation: Reconciliation };

export type Result =
  | { kind: 'outcome'; delivery: Delivery; replayed: boolean; recovered: boolean }
  | { kind: 'in_progress'; state: RecordState | null }
  | { kind: 'conflicting_reuse'; differs: ('target' | 'mode' | 'text' | 'attachments')[] }
  | { kind: 'possible_duplicate_refused'; earlier: EarlierSend[] };


export type PromptAttachment = { id: string; sha256: string; size_bytes: number; kind: 'image' | 'file'; name: string };

export type Report = {
  schema: typeof REPORT_SCHEMA_ID;
  op_id: string;
  target: Target;
  source: Source | null;
  requested_mode: Mode;
  status: 'queued' | 'accepted' | 'failed' | 'uncertain' | 'in_progress' | 'conflicting_reuse' | 'possible_duplicate_refused';
  summary: string;
  result: Result;
  effects: Effects | null;
  text_sha256: string;
  text_bytes: number;
  /** B2: absent when there are no attachments. */
  attachments_digest?: string;
  /** B2: absent when there are no attachments. */
  attachments?: PromptAttachment[];
  gaps: Gap[];
  claimed_at: number | null;
  dispatching_at: number | null;
  finished_at: number | null;
};

export type Refusal = { reason: FailReason; text: string };

export type Preview = {
  schema: typeof PREVIEW_SCHEMA_ID;
  target: Target;
  requested_mode: Mode;
  status: 'ready' | 'needs_acknowledgement' | 'refused';
  summary: string;
  effects: Effects | null;
  refusal: Refusal | null;
  read_at: number;
  gaps: Gap[];
};

export type Lookup = {
  schema: typeof LOOKUP_SCHEMA_ID;
  op_id: string;
  target: string;
  summary: string;
  result: { kind: 'no_record' } | { kind: 'other_target' } | { kind: 'recorded'; report: Report };
};

// Runtime checks, exact to the schema.

const text: Check = (v) => str(v) && (v as string).length >= 1;
const opId: Check = (v) => str(v) && /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(v as string);
const mode = is('queue', 'steer');
const failReason = is(...FAIL_REASONS);
const recordState = is('claimed', 'dispatching', 'finished');
const unknownWhy = exact({ unknown: text });
const targetCheck = exact({ id: text, title: fact(str) });
const sourceCheck = exact({ name: str, version: fact(str), tested_version: str, version_tested: bool });
const gapCheck = exact({ subject: text, detail: text });
const effectCheck = exact({
  kind: is('starts_turn', 'queues_behind_turn', 'asks_to_steer', 'waits_for_interaction', 'waits_for_host'),
  text,
});
const pausedCheck = exact({ appears_paused: bool, queued_messages: fact(count), text });
const effectsCheck = exact({
  read_at: int,
  read_age_ms: int,
  effect: oneOf(exact({ known: effectCheck }), unknownWhy),
  paused_queue: oneOf(exact({ known: pausedCheck }), unknownWhy),
  needs_acknowledgement: bool,
});
const reconciliationCheck = oneOf(
  exact({ kind: is('not_read') }),
  exact({ kind: is('observed'), matching: count, total: count }),
  exact({ kind: is('attributed') }),
);
const deliveryCheck = oneOf(
  exact({
    kind: is('queued'),
    waiting_on: is('not_reported', 'time', 'thread_busy', 'turn_starting', 'provisioning', 'host_offline', 'interaction', 'plugin'),
    queued_message_id: (v) => text(v) && (v as string).length <= 128,
    text,
  }),
  exact({ kind: is('accepted'), joined_running_turn: fact(bool), text }),
  exact({ kind: is('failed'), reason: failReason, text }),
  exact({
    kind: is('uncertain'),
    reason: is(
      'timed_out',
      'ended_without_exit_code',
      'not_json',
      'unexpected_receipt',
      'undocumented_error',
      'call_not_confirmed',
      'interrupted_after_dispatch_mark',
    ),
    text,
    reconciliation: reconciliationCheck,
    attribution: text,
  }),
);
const earlierCheck = exact({ op_id: opId, mode, claimed_at: int, in_flight: bool, reconciliation: reconciliationCheck });
const resultCheck = oneOf(
  exact({ kind: is('outcome'), delivery: deliveryCheck, replayed: bool, recovered: bool }),
  exact({ kind: is('in_progress'), state: oneOf(isNull, recordState) }),
  exact({ kind: is('conflicting_reuse'), differs: list(is('target', 'mode', 'text', 'attachments')) }),
  exact({ kind: is('possible_duplicate_refused'), earlier: list(earlierCheck) }),
);
const nullable = (check: Check): Check => oneOf(isNull, check);
const sha256Check: Check = (v) => str(v) && /^[0-9a-f]{64}$/.test(v as string);
/** One attachment of a send, as its record keeps it (B2): never its bytes. */
const promptAttachmentCheck = exact({
  id: (v) => str(v) && /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(v as string),
  sha256: sha256Check,
  size_bytes: (v) => int(v) && (v as number) >= 0,
  kind: is('image', 'file'),
  name: (v) => str(v) && (v as string).length >= 1 && (v as string).length <= 179,
});
const reportCheck = exactWith({
  schema: is(REPORT_SCHEMA_ID),
  op_id: opId,
  target: targetCheck,
  source: nullable(sourceCheck),
  requested_mode: mode,
  status: is('queued', 'accepted', 'failed', 'uncertain', 'in_progress', 'conflicting_reuse', 'possible_duplicate_refused'),
  summary: text,
  result: resultCheck,
  effects: nullable(effectsCheck),
  text_sha256: (v) => str(v) && /^[0-9a-f]{64}$/.test(v as string),
  text_bytes: (v) => int(v) && (v as number) >= 1,
  gaps: list(gapCheck),
  claimed_at: nullable(int),
  dispatching_at: nullable(int),
  finished_at: nullable(int),
}, { attachments_digest: sha256Check, attachments: list(promptAttachmentCheck) });
const previewCheck = exact({
  schema: is(PREVIEW_SCHEMA_ID),
  target: targetCheck,
  requested_mode: mode,
  status: is('ready', 'needs_acknowledgement', 'refused'),
  summary: text,
  effects: nullable(effectsCheck),
  refusal: nullable(exact({ reason: failReason, text })),
  read_at: int,
  gaps: list(gapCheck),
});
const lookupCheck = exact({
  schema: is(LOOKUP_SCHEMA_ID),
  op_id: opId,
  target: text,
  summary: text,
  result: oneOf(exact({ kind: is('no_record') }), exact({ kind: is('other_target') }), exact({ kind: is('recorded'), report: reportCheck })),
});

/** A send report, or null when `json` is not exactly one. */
export function parseReport(json: unknown): Report | null {
  return reportCheck(json) ? (json as Report) : null;
}

/** A preview, or null when `json` is not exactly one. */
export function parsePreview(json: unknown): Preview | null {
  return previewCheck(json) ? (json as Preview) : null;
}

/** A lookup, or null when `json` is not exactly one. */
export function parseLookup(json: unknown): Lookup | null {
  return lookupCheck(json) ? (json as Lookup) : null;
}
