// The `openwiz.inventory.v1` envelope, as `contracts/inventory.v1.schema.json`
// defines it. One type per schema definition, named after it in PascalCase,
// with exactly the schema's property names: `npm run check` compares them.
// The web reads nothing else.

export const SCHEMA_ID = 'openwiz.inventory.v1';

export type Unknown = { unknown: string };
export type Fact<T> = { known: T } | Unknown;
export type FactString = Fact<string>;
export type FactBoolean = Fact<boolean>;
export type FactInteger = Fact<number>;

export type Source = {
  name: string;
  version: FactString;
  tested_version: string;
  version_tested: boolean;
};

export type Counts = {
  listed: number;
  unreadable: number;
  excluded_hidden: number;
  excluded_archived: number;
  hidden_read: boolean;
  archived_read: boolean;
  matched: number;
  offset: number;
  limit: number;
  shown: number;
};

export type Coverage = {
  state: 'complete' | 'partial';
  counts: Counts | null;
};

export type Freshness = { snapshot_at: number };

export const GAP_KINDS = [
  'untested_version',
  'command_failed',
  'timed_out',
  'budget_exhausted',
  'malformed',
  'missing_field',
  'wrong_type',
  'not_found',
] as const;

export type Gap = {
  kind: (typeof GAP_KINDS)[number];
  subject: string | null;
  detail: string;
};

export type Location = { kind: 'cloud' } | { kind: 'local'; machine: FactString };

export type Grouping =
  | { kind: 'project'; id: string; name: FactString }
  | { kind: 'ungrouped'; reason: string };

export type BoundedText = { text: string; truncated: boolean };

export type Recap = { kind: 'purpose_as_reported'; text: string; truncated: boolean } | { kind: 'no_recap_yet' };

export type FinalOutput = { kind: 'reported'; text: string; truncated: boolean } | { kind: 'no_final_output_yet' };

export type ReportedConfiguration = { model: FactString; effort: FactString };

export type Session = {
  id: string;
  title: FactString;
  provider: FactString;
  status: FactString;
  location: Fact<Location>;
  group: Grouping;
  parent: Fact<string | null>;
  hidden: FactBoolean;
  archived: FactBoolean;
  recap: Fact<Recap>;
  reported_configuration: ReportedConfiguration;
  updated_at: FactInteger;
  age_ms: FactInteger;
};

export type Inventory = { sessions: Session[] };

export type SessionView = {
  session: Session;
  final_output: Fact<FinalOutput>;
  reported_error: Fact<BoundedText | null>;
};

export type Envelope<D = Inventory | SessionView> = {
  schema: typeof SCHEMA_ID;
  source: Source;
  coverage: Coverage;
  freshness: Freshness;
  gaps: Gap[];
  data: D;
};

/** The web API's error body (`contracts/web-api.v1.md`, "Error body"). */
export type ApiError = { error: { kind: string; message: string } };

// Runtime checks, exact to the schema: every required key, no other key, the
// right types and enums. A response that fails is not shown as data.

type Check = (v: unknown) => boolean;

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);

const exact =
  (shape: Record<string, Check>): Check =>
  (v) =>
    isObject(v) &&
    Object.keys(v).length === Object.keys(shape).length &&
    Object.entries(shape).every(([key, check]) => Object.hasOwn(v, key) && check(v[key]));

/** JSON Schema's oneOf: exactly one matches. */
const oneOf =
  (...checks: Check[]): Check =>
  (v) =>
    checks.filter((check) => check(v)).length === 1;

const str: Check = (v) => typeof v === 'string';
const bool: Check = (v) => typeof v === 'boolean';
const int: Check = (v) => typeof v === 'number' && Number.isSafeInteger(v);
const count: Check = (v) => int(v) && (v as number) >= 0;
const isNull: Check = (v) => v === null;
const is =
  (...values: readonly unknown[]): Check =>
  (v) =>
    values.includes(v);
const list =
  (check: Check): Check =>
  (v) =>
    Array.isArray(v) && v.every(check);

const unknownCheck = exact({ unknown: (v) => str(v) && (v as string).length >= 1 });
const fact = (check: Check): Check => oneOf(exact({ known: check }), unknownCheck);

const sourceCheck = exact({ name: str, version: fact(str), tested_version: str, version_tested: bool });
const countsCheck = exact({
  listed: count,
  unreadable: count,
  excluded_hidden: count,
  excluded_archived: count,
  hidden_read: bool,
  archived_read: bool,
  matched: count,
  offset: count,
  limit: count,
  shown: count,
});
const coverageCheck = exact({ state: is('complete', 'partial'), counts: oneOf(isNull, countsCheck) });
const freshnessCheck = exact({ snapshot_at: int });
const gapCheck = exact({ kind: is(...GAP_KINDS), subject: oneOf(isNull, str), detail: str });
const locationCheck = oneOf(exact({ kind: is('cloud') }), exact({ kind: is('local'), machine: fact(str) }));
const groupingCheck = oneOf(
  exact({ kind: is('project'), id: str, name: fact(str) }),
  exact({ kind: is('ungrouped'), reason: str }),
);
const recapCheck = oneOf(
  exact({ kind: is('purpose_as_reported'), text: (v) => str(v) && [...(v as string)].length <= 120, truncated: bool }),
  exact({ kind: is('no_recap_yet') }),
);
const finalOutputCheck = oneOf(exact({ kind: is('reported'), text: str, truncated: bool }), exact({ kind: is('no_final_output_yet') }));
const boundedTextCheck = exact({ text: str, truncated: bool });
const sessionCheck = exact({
  id: str,
  title: fact(str),
  provider: fact(str),
  status: fact(str),
  location: fact(locationCheck),
  group: groupingCheck,
  parent: fact(oneOf(isNull, str)),
  hidden: fact(bool),
  archived: fact(bool),
  recap: fact(recapCheck),
  reported_configuration: exact({ model: fact(str), effort: fact(str) }),
  updated_at: fact(int),
  age_ms: fact(int),
});
const envelopeCheck = (data: Check): Check =>
  exact({ schema: is(SCHEMA_ID), source: sourceCheck, coverage: coverageCheck, freshness: freshnessCheck, gaps: list(gapCheck), data });

const inventoryEnvelope = envelopeCheck(exact({ sessions: list(sessionCheck) }));
const sessionViewEnvelope = envelopeCheck(
  exact({ session: sessionCheck, final_output: fact(finalOutputCheck), reported_error: fact(oneOf(isNull, boundedTextCheck)) }),
);
const apiErrorCheck = exact({ error: exact({ kind: str, message: str }) });

/** An inventory envelope, or null when `json` is not exactly one. */
export function parseInventory(json: unknown): Envelope<Inventory> | null {
  return inventoryEnvelope(json) ? (json as Envelope<Inventory>) : null;
}

/** A session envelope (`show`), or null when `json` is not exactly one. */
export function parseSessionView(json: unknown): Envelope<SessionView> | null {
  return sessionViewEnvelope(json) ? (json as Envelope<SessionView>) : null;
}

/** The API's error body, or null when `json` is not exactly one. */
export function parseApiError(json: unknown): ApiError | null {
  return apiErrorCheck(json) ? (json as ApiError) : null;
}
