// The new-thread shapes the launcher reads , as
// `contracts/new-thread.v1.schema.json` defines them: the Cloud options
// (`openwiz models --json`), the preview (`openwiz new … --json`), the report
// (`openwiz new … --yes --json`) and the status (`openwiz new --status`). One
// type per schema definition it reads, named after it in PascalCase, with
// exactly the schema's property names: `npm run check` compares them. The web
// works out nothing from them but words, and which options to list.

import { bool, count, exact, exactWith, fact, int, is, isNull, list, oneOf, str } from './envelope.ts';
import type { Check, Fact, FactString } from './envelope.ts';

export const OPTIONS_SCHEMA_ID = 'openwiz.cloud-options.v1';
export const PREVIEW_SCHEMA_ID = 'openwiz.new-thread-preview.v1';
export const REPORT_SCHEMA_ID = 'openwiz.new-thread.v1';
export const STATUS_SCHEMA_ID = 'openwiz.new-thread-status.v1';

export type Gap = { subject: string; detail: string };

export type Source = { name: string; version: FactString; tested_version: string; version_tested: boolean };

export type ProjectRef = { kind: 'id' | 'name'; value: string };

export type ProjectView = { id: string; name: FactString };

/** The requested configuration: what the owner chose. Never what CloudRoom reports. */
export type Settings = { harness: string; model: string; effort: string };

export type AttachmentSupport = { files: boolean; images: boolean };

export type ModelOption = { model: string; efforts: string[] };

export type HarnessOption = { harness: string; provider: string; models: ModelOption[]; attachments: AttachmentSupport | null; steer: boolean | null };

export type UnavailableHarness = { harness: string; reason: 'catalogue_not_reported' | 'id_mapping_not_verified' | 'no_offered_effort'; text: string };

export type Options = {
  schema: typeof OPTIONS_SCHEMA_ID;
  read_at: number;
  label: string;
  state: { kind: 'offered' } | { kind: 'no_options'; reason: 'not_ready' | 'harnesses_not_reported' | 'read_failed' | 'unexpected_shape'; text: string };
  harnesses: HarnessOption[];
  unavailable: UnavailableHarness[];
};

export const EFFECT_KINDS = [
  'creates_and_starts',
  'starts_from_github',
  'copies_env_files',
  'full_permissions',
  'uses_cloud_agent',
  'mac_stays_awake',
  'cloud_room_names_it',
] as const;

export type Effect = { kind: (typeof EFFECT_KINDS)[number]; text: string };

export type Effects = { read_at: number; project: ProjectView; effects: Effect[] };

export const REFUSAL_REASONS = [
  'no_options',
  'harness_not_offered',
  'model_not_offered',
  'effort_not_offered',
  'project_not_found',
  'project_ambiguous',
  'project_personal',
  'project_not_standard',
  'project_no_github_remote',
  'project_remote_not_confirmed',
  'provider_unsupported',
  'provider_model_unavailable',
  'provider_effort_unavailable',
  'provider_message_too_long',
  'provider_message_empty',
  'provider_project_not_found',
  'provider_request_id_conflict',
  'provider_discovery_unavailable',
] as const;

export const FAIL_REASONS = ['interrupted_before_dispatch', 'projects_not_read', 'record_not_written', 'budget_exhausted', 'call_not_started'] as const;

export const UNCERTAIN_REASONS = [
  'timed_out',
  'ended_without_exit_code',
  'not_json',
  'unexpected_receipt',
  'undocumented_error',
  'call_not_confirmed',
  'interrupted_after_dispatch_mark',
] as const;

/** `created` means CloudRoom recorded the thread and its first prompt, never that it started. */
export type Outcome =
  | { kind: 'created'; thread_id: string; text: string }
  | { kind: 'refused'; reason: (typeof REFUSAL_REASONS)[number]; offered: string[]; text: string }
  | { kind: 'failed'; reason: (typeof FAIL_REASONS)[number]; text: string }
  | { kind: 'uncertain'; reason: (typeof UNCERTAIN_REASONS)[number]; text: string };

export type BindingField = 'project' | 'harness' | 'model' | 'effort' | 'text' | 'attachments';

export type Result =
  | { kind: 'outcome'; outcome: Outcome; replayed: boolean; recovered: boolean }
  | { kind: 'in_progress'; state: 'claimed' | 'dispatching' | 'finished' | null }
  | { kind: 'conflicting_reuse'; differs: BindingField[] };

export type StartStage =
  | { kind: 'waiting_to_start'; text: string }
  | { kind: 'started'; text: string }
  | { kind: 'idle_as_reported'; text: string }
  | { kind: 'start_failed'; last_error: FactString; text: string }
  | { kind: 'unknown'; reason: string; text: string };

export type ReportedSetting = { requested: string; reported: FactString; differs_from_requested: Fact<boolean>; text: string };

export type ReportedConfiguration = { label: string; provider: ReportedSetting; model: ReportedSetting; effort: ReportedSetting };

export type StartView = { thread_id: string; read_at: number; stage: StartStage; reported: ReportedConfiguration | null; gaps: Gap[] };


export type PromptAttachment = { id: string; sha256: string; size_bytes: number; kind: 'image' | 'file'; name: string };

export type Report = {
  schema: typeof REPORT_SCHEMA_ID;
  op_id: string;
  request_id: string;
  project_ref: ProjectRef;
  project: ProjectView | null;
  source: Source | null;
  requested: Settings;
  provider: string | null;
  status: 'created' | 'refused' | 'failed' | 'uncertain' | 'in_progress' | 'conflicting_reuse';
  summary: string;
  result: Result;
  effects: Effects | null;
  text_sha256: string;
  attachments_digest: string;
  /** B2: absent when there are no attachments. */
  attachments?: PromptAttachment[];
  text_bytes: number;
  gaps: Gap[];
  claimed_at: number | null;
  dispatching_at: number | null;
  finished_at: number | null;
  start: StartView | null;
};

export type Preview = {
  schema: typeof PREVIEW_SCHEMA_ID;
  op_id: string;
  request_id: string;
  project_ref: ProjectRef;
  project: ProjectView | null;
  source: Source;
  requested: Settings;
  verdict: { kind: 'ready'; provider: string } | { kind: 'stopped'; outcome: Outcome };
  effects: Effects | null;
  text_sha256: string;
  attachments_digest: string;
  /** B2: absent when there are no attachments. */
  attachments?: PromptAttachment[];
  text_bytes: number;
  gaps: Gap[];
};

export type ObservedThread = { id: string; created_at: Fact<number> };

export type Observations = { read_at: number; threads: ObservedThread[]; text: string; gaps: Gap[] };

export type StatusRead = {
  schema: typeof STATUS_SCHEMA_ID;
  op_id: string;
  result: { kind: 'no_record' } | { kind: 'recorded'; report: Report; start: StartView | null; observations: Observations | null };
};

// Runtime checks, exact to the schema.

const text: Check = (v) => str(v) && (v as string).length >= 1;
const opId: Check = (v) => str(v) && /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(v as string);
const sha256: Check = (v) => str(v) && /^[0-9a-f]{64}$/.test(v as string);
/** One attachment of a first prompt, as its record keeps it (B2): never its bytes. */
const promptAttachmentCheck = exact({
  id: (v) => str(v) && /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(v as string),
  sha256,
  size_bytes: (v) => int(v) && (v as number) >= 0,
  kind: is('image', 'file'),
  name: (v) => str(v) && (v as string).length >= 1 && (v as string).length <= 179,
});
const requestId: Check = (v) => str(v) && /^ow1-[0-9a-f]{40}$/.test(v as string);
const setting: Check = (v) => str(v) && /^[!-,.-~][!-~]{0,127}$/.test(v as string);
const nullable = (check: Check): Check => oneOf(isNull, check);
const gapCheck = exact({ subject: text, detail: text });
const sourceCheck = exact({ name: str, version: fact(str), tested_version: str, version_tested: bool });
const projectRefCheck = exact({ kind: is('id', 'name'), value: text });
const projectViewCheck = exact({ id: text, name: fact(str) });
const settingsCheck = exact({ harness: setting, model: setting, effort: setting });
const attachmentCheck = exact({ files: bool, images: bool });
const modelCheck = exact({ model: text, efforts: list(text) });
const harnessCheck = exact({ harness: text, provider: text, models: list(modelCheck), attachments: nullable(attachmentCheck), steer: nullable(bool) });
const unavailableCheck = exact({ harness: text, reason: is('catalogue_not_reported', 'id_mapping_not_verified', 'no_offered_effort'), text });
const optionsCheck = exact({
  schema: is(OPTIONS_SCHEMA_ID),
  read_at: int,
  label: text,
  state: oneOf(exact({ kind: is('offered') }), exact({ kind: is('no_options'), reason: is('not_ready', 'harnesses_not_reported', 'read_failed', 'unexpected_shape'), text })),
  harnesses: list(harnessCheck),
  unavailable: list(unavailableCheck),
});
const effectCheck = exact({ kind: is(...EFFECT_KINDS), text });
const effectsCheck = exact({ read_at: int, project: projectViewCheck, effects: list(effectCheck) });
const outcomeCheck = oneOf(
  exact({ kind: is('created'), thread_id: text, text }),
  exact({ kind: is('refused'), reason: is(...REFUSAL_REASONS), offered: list(str), text }),
  exact({ kind: is('failed'), reason: is(...FAIL_REASONS), text }),
  exact({ kind: is('uncertain'), reason: is(...UNCERTAIN_REASONS), text }),
);
const resultCheck = oneOf(
  exact({ kind: is('outcome'), outcome: outcomeCheck, replayed: bool, recovered: bool }),
  exact({ kind: is('in_progress'), state: nullable(is('claimed', 'dispatching', 'finished')) }),
  exact({ kind: is('conflicting_reuse'), differs: list(is('project', 'harness', 'model', 'effort', 'text', 'attachments')) }),
);
const stageCheck = oneOf(
  exact({ kind: is('waiting_to_start'), text }),
  exact({ kind: is('started'), text }),
  exact({ kind: is('idle_as_reported'), text }),
  exact({ kind: is('start_failed'), last_error: fact(str), text }),
  exact({ kind: is('unknown'), reason: text, text }),
);
const reportedSettingCheck = exact({ requested: text, reported: fact(str), differs_from_requested: fact(bool), text });
const reportedCheck = exact({ label: text, provider: reportedSettingCheck, model: reportedSettingCheck, effort: reportedSettingCheck });
const startCheck = exact({ thread_id: text, read_at: int, stage: stageCheck, reported: nullable(reportedCheck), gaps: list(gapCheck) });
const reportCheck = exactWith({
  schema: is(REPORT_SCHEMA_ID),
  op_id: opId,
  request_id: requestId,
  project_ref: projectRefCheck,
  project: nullable(projectViewCheck),
  source: nullable(sourceCheck),
  requested: settingsCheck,
  provider: nullable(text),
  status: is('created', 'refused', 'failed', 'uncertain', 'in_progress', 'conflicting_reuse'),
  summary: text,
  result: resultCheck,
  effects: nullable(effectsCheck),
  text_sha256: sha256,
  attachments_digest: sha256,
  text_bytes: count,
  gaps: list(gapCheck),
  claimed_at: nullable(int),
  dispatching_at: nullable(int),
  finished_at: nullable(int),
  start: nullable(startCheck),
}, { attachments: list(promptAttachmentCheck) });
const previewCheck = exactWith({
  schema: is(PREVIEW_SCHEMA_ID),
  op_id: opId,
  request_id: requestId,
  project_ref: projectRefCheck,
  project: nullable(projectViewCheck),
  source: sourceCheck,
  requested: settingsCheck,
  verdict: oneOf(exact({ kind: is('ready'), provider: text }), exact({ kind: is('stopped'), outcome: outcomeCheck })),
  effects: nullable(effectsCheck),
  text_sha256: sha256,
  attachments_digest: sha256,
  text_bytes: count,
  gaps: list(gapCheck),
}, { attachments: list(promptAttachmentCheck) });
const observedCheck = exact({ id: text, created_at: fact(int) });
const observationsCheck = exact({ read_at: int, threads: list(observedCheck), text, gaps: list(gapCheck) });
const statusCheck = exact({
  schema: is(STATUS_SCHEMA_ID),
  op_id: opId,
  result: oneOf(
    exact({ kind: is('no_record') }),
    exact({ kind: is('recorded'), report: reportCheck, start: nullable(startCheck), observations: nullable(observationsCheck) }),
  ),
});

/** The Cloud options, or null when `json` is not exactly them. */
export function parseOptions(json: unknown): Options | null {
  return optionsCheck(json) ? (json as Options) : null;
}

/** A new-thread preview, or null when `json` is not exactly one. */
export function parsePreview(json: unknown): Preview | null {
  return previewCheck(json) ? (json as Preview) : null;
}

/** A new-thread report, or null when `json` is not exactly one. */
export function parseReport(json: unknown): Report | null {
  return reportCheck(json) ? (json as Report) : null;
}

/** A status read, or null when `json` is not exactly one. */
export function parseStatus(json: unknown): StatusRead | null {
  return statusCheck(json) ? (json as StatusRead) : null;
}
