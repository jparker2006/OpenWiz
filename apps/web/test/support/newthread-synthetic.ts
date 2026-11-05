// Synthetic new-thread answers for the launcher's proofs : the Cloud
// options, the preview, the report and the status, shaped exactly as
// contracts/new-thread.v1.schema.json says (the page's own exact checks read
// them). Every value is invented: projects are `prj_web_NNNN`, threads
// `thr_web_9NNN`, models `syn-…`. The words are the core's (crates/core's
// new_thread), copied so the screens read as the real ones do; the real
// server's answers are proven against the same checks in u-real-server.

import type { Choice } from '../../src/launcher.ts';
import type { Effects, Options, Outcome, Preview, Report, Settings, StartStage, StartView, StatusRead } from '../../src/newthread.ts';
import { sha256, T0 } from './synthetic.ts';

export const EFFORTS = ['low', 'medium', 'high', 'xhigh', 'max'];

/** The options CloudRoom offers in the proofs: two harnesses, and one listed but unavailable. Nothing is marked default. */
export function options(readAt = T0, over: Partial<Options> = {}): Options {
  return {
    schema: 'openwiz.cloud-options.v1',
    read_at: readAt,
    label: 'as reported by CloudRoom; it may answer from a saved copy',
    state: { kind: 'offered' },
    harnesses: [
      {
        harness: 'codex',
        provider: 'codex',
        models: [
          { model: 'syn-codex-model-01', efforts: [...EFFORTS] },
          { model: 'syn-codex-model-02', efforts: [...EFFORTS] },
          { model: 'syn-shared-model', efforts: ['low', 'medium', 'high'] },
        ],
        attachments: { files: true, images: true },
        steer: true,
      },
      {
        harness: 'claude-code',
        provider: 'claude-code',
        models: [
          { model: 'syn-claude-model-01', efforts: ['low', 'medium', 'high', 'xhigh', 'max'] },
          { model: 'syn-shared-model', efforts: ['medium', 'max'] },
        ],
        attachments: { files: true, images: true },
        steer: true,
      },
    ],
    unavailable: [{ harness: 'cursor', reason: 'catalogue_not_reported', text: 'unavailable (catalogue not reported)' }],
    ...over,
  };
}

/** No options at all: CloudRoom isn't ready. */
export const noOptions = (): Options => options(T0, { state: { kind: 'no_options', reason: 'not_ready', text: 'CloudRoom says Cloud is not ready, so nothing can be offered' }, harnesses: [], unavailable: [] });

export const PROJECT_NAME = 'Synthetic web project one';

/** The core's effect lines for a create in a project named `name`. */
export function effects(project: string, name = PROJECT_NAME, readAt = T0): Effects {
  const e = (kind: Effects['effects'][number]['kind'], text: string) => ({ kind, text });
  return {
    read_at: readAt,
    project: { id: project, name: { known: name } },
    effects: [
      e('creates_and_starts', `creates one Cloud thread in ${name} and starts its first turn`),
      e('starts_from_github', "starts from the project's default branch on GitHub"),
      e('copies_env_files', "copies the project's .env files from this computer"),
      e('full_permissions', 'runs with full permissions in its own sandbox'),
      e('uses_cloud_agent', 'uses one Cloud agent from your plan'),
      e('mac_stays_awake', 'this computer must stay awake until the start is confirmed'),
      e('cloud_room_names_it', 'CloudRoom names it'),
    ],
  };
}

export type CreateBody = Choice & { op_id: string; text: string; attachments: string[] };

const source = { name: 'room-cli', version: { known: 'v131' }, tested_version: 'v131', version_tested: true };

const settingsOf = (b: CreateBody): Settings => ({ harness: b.harness, model: b.model, effort: b.effort });

export const CREATED_WORDS = 'created: CloudRoom recorded the thread and its first prompt. That is not a start: the start stage is shown separately';

export const OUTCOMES = {
  created: (threadId: string): Outcome => ({ kind: 'created', thread_id: threadId, text: CREATED_WORDS }),
  notOffered: (offered: string[]): Outcome => ({ kind: 'refused', reason: 'model_not_offered', offered, text: 'refused: the model is not offered for this harness; nothing was created' }),
  noRemote: (): Outcome => ({ kind: 'refused', reason: 'project_no_github_remote', offered: [], text: 'refused: the project has no GitHub remote, and only projects with one are accepted; nothing was created' }),
  failed: (): Outcome => ({ kind: 'failed', reason: 'projects_not_read', text: 'failed: the project list could not be read; nothing was sent' }),
  uncertain: (): Outcome => ({
    kind: 'uncertain',
    reason: 'timed_out',
    text: 'uncertain: the provider did not answer in time. The thread may or may not have been created; the prompt is kept and nothing is retried',
  }),
};

/** The core's start view of `threadId`, as one `thread show` read it. */
export function start(threadId: string, stage: StartStage['kind'], b: Settings, readAt = T0 + 20, reportedModel: string | null = null): StartView {
  const stages: Record<StartStage['kind'], StartStage> = {
    waiting_to_start: { kind: 'waiting_to_start', text: 'waiting to start' },
    started: { kind: 'started', text: 'started: its first turn is running, as reported' },
    idle_as_reported: { kind: 'idle_as_reported', text: 'idle, as reported: whether the first prompt ran is not confirmed' },
    start_failed: { kind: 'start_failed', last_error: { known: 'Synthetic start error' }, text: "start failed, in CloudRoom's words below; the prompt stays saved in OpenWiz's record" },
    unknown: { kind: 'unknown', reason: 'thread show could not be read', text: 'unknown (thread show could not be read)' },
  };
  const setting = (name: string, requested: string, reported: string) => ({
    requested,
    reported: { known: reported },
    differs_from_requested: { known: requested !== reported },
    text: requested === reported ? `${name} ${reported}, as requested` : `${name} ${reported}: differs from requested (${requested})`,
  });
  return {
    thread_id: threadId,
    read_at: readAt,
    stage: stages[stage],
    reported: {
      label: 'reported configuration: what CloudRoom reports, never the serving model',
      provider: setting('provider', b.harness, b.harness),
      model: setting('model', b.model, reportedModel ?? b.model),
      effort: setting('effort', b.effort, b.effort),
    },
    gaps: [],
  };
}

const STATUS: Record<Outcome['kind'], Report['status']> = { created: 'created', refused: 'refused', failed: 'failed', uncertain: 'uncertain' };

/** The CLI's report of a create of `b` in `project`, ending in `outcome`. */
export function report(project: string, b: CreateBody, outcome: Outcome, startView: StartView | null = null, readAt = T0): Report {
  const s = settingsOf(b);
  return {
    schema: 'openwiz.new-thread.v1',
    op_id: b.op_id,
    request_id: `ow1-${sha256(`openwiz.new-thread.request-id.v1:${b.op_id}`).slice(0, 40)}`,
    project_ref: { kind: 'id', value: project },
    project: { id: project, name: { known: PROJECT_NAME } },
    source,
    requested: s,
    provider: outcome.kind === 'refused' ? null : b.harness,
    status: STATUS[outcome.kind],
    summary: `${s.harness} · ${s.model} · ${s.effort} requested · ${outcome.text}`,
    result: { kind: 'outcome', outcome, replayed: false, recovered: false },
    effects: outcome.kind === 'refused' ? null : effects(project, PROJECT_NAME, readAt),
    text_sha256: sha256(b.text),
    attachments_digest: sha256(''),
    text_bytes: Buffer.byteLength(b.text, 'utf8'),
    gaps: [],
    claimed_at: readAt,
    dispatching_at: outcome.kind === 'refused' ? null : readAt + 5,
    finished_at: readAt + 10,
    start: outcome.kind === 'created' ? startView : null,
  };
}

/** The core's preview of `b` in `project`: ready, or the outcome that would stop it. */
export function preview(project: string, b: CreateBody, stopped: Outcome | null = null, readAt = T0): Preview {
  return {
    schema: 'openwiz.new-thread-preview.v1',
    op_id: b.op_id,
    request_id: `ow1-${sha256(`openwiz.new-thread.request-id.v1:${b.op_id}`).slice(0, 40)}`,
    project_ref: { kind: 'id', value: project },
    project: { id: project, name: { known: PROJECT_NAME } },
    source,
    requested: settingsOf(b),
    verdict: stopped ? { kind: 'stopped', outcome: stopped } : { kind: 'ready', provider: b.harness },
    effects: stopped ? null : effects(project, PROJECT_NAME, readAt),
    text_sha256: sha256(b.text),
    attachments_digest: sha256(''),
    text_bytes: Buffer.byteLength(b.text, 'utf8'),
    gaps: [],
  };
}

/** The status read of a recorded report: replayed, with its start or observations beside it. */
export function status(opId: string, recorded: Report | null, startView: StartView | null = null, seen = 0): StatusRead {
  if (!recorded) return { schema: 'openwiz.new-thread-status.v1', op_id: opId, result: { kind: 'no_record' } };
  const replayed: Report = { ...recorded, start: null, result: recorded.result.kind === 'outcome' ? { ...recorded.result, replayed: true } : recorded.result };
  const uncertain = recorded.status === 'uncertain';
  return {
    schema: 'openwiz.new-thread-status.v1',
    op_id: opId,
    result: {
      kind: 'recorded',
      report: replayed,
      start: recorded.status === 'created' ? (startView ?? recorded.start) : null,
      observations: uncertain
        ? {
            read_at: T0 + 30,
            threads: Array.from({ length: seen }, (_, i) => ({ id: `thr_web_9${String(i + 1).padStart(3, '0')}`, created_at: { known: T0 + i } })),
            text: 'observations only: no documented field ties a listed thread to this request, so the outcome stays uncertain',
            gaps: [],
          }
        : null,
    },
  };
}
