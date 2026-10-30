// What the page must show for an envelope (V1), written independently of the
// app's own code: the CLI's text rules (crates/cli/src/lib.rs) and the
// page's own wording, nothing more.

export const unknown = (reason) => `unknown (${reason})`;
export const fact = (f) => ('known' in f ? f.known : unknown(f.unknown));
export const bounded = (text, truncated) => (truncated ? `${text}…` : text);

/** The CLI's `age()`. */
export function age(ms) {
  const sign = ms < 0 ? '-' : '';
  const s = Math.floor(Math.abs(ms) / 1000);
  for (const [unit, size] of [
    ['d', 86_400],
    ['h', 3_600],
    ['m', 60],
  ]) {
    if (s >= size) return `${sign}${Math.floor(s / size)}${unit}`;
  }
  return `${sign}${s}s`;
}

/** Device-local `HH:MM:SS` (the proofs pin the browser to UTC), with the date on another day. */
export function clock(epochMs, nowMs) {
  const d = new Date(epochMs);
  const n = new Date(nowMs);
  const two = (v) => String(v).padStart(2, '0');
  const time = `${two(d.getUTCHours())}:${two(d.getUTCMinutes())}:${two(d.getUTCSeconds())}`;
  const sameDay = d.getUTCFullYear() === n.getUTCFullYear() && d.getUTCMonth() === n.getUTCMonth() && d.getUTCDate() === n.getUTCDate();
  return sameDay ? time : `${d.getUTCFullYear()}-${two(d.getUTCMonth() + 1)}-${two(d.getUTCDate())} ${time}`;
}

const TIERS = { error: ['t1', '[x]'], active: ['t3', '[~]'], idle: ['t4', '[-]'] };

export function status(f) {
  if (!('known' in f)) return { word: unknown(f.unknown), glyph: '[?]', tier: 't5' };
  const [tier, glyph] = TIERS[f.known] ?? ['t5', '[?]'];
  return { word: f.known, glyph, tier };
}

const groupKey = (g) => (g.kind === 'project' ? `project:${g.id}` : 'ungrouped');

/** One page under its group headings: first-appearance order, page order kept. */
export function groups(sessions) {
  const out = [];
  for (const s of sessions) {
    const key = groupKey(s.group);
    let g = out.find((x) => x.key === key);
    if (!g) {
      g = { key, heading: s.group.kind === 'project' ? fact(s.group.name) : 'ungrouped', rows: [] };
      out.push(g);
    }
    g.rows.push(row(s));
  }
  return out;
}

export function row(s) {
  return {
    id: s.id,
    title: fact(s.title),
    status: status(s.status),
    age: 'known' in s.age_ms ? age(s.age_ms.known) : unknown(s.age_ms.unknown),
    reason: s.group.kind === 'ungrouped' ? s.group.reason : null,
  };
}

export const gapLine = (g) => `${g.kind}${g.subject === null ? '' : ` · ${g.subject}`} · ${g.detail}`;

export const coverage = (e) => (e.coverage.state === 'complete' ? 'complete' : `partial · ${e.gaps.length} ${e.gaps.length === 1 ? 'gap' : 'gaps'}`);

export function location(l) {
  if (!('known' in l)) return unknown(l.unknown);
  return l.known.kind === 'cloud' ? 'cloud' : `local on ${fact(l.known.machine)}`;
}

export function purpose(r) {
  if (!('known' in r)) return unknown(r.unknown);
  return r.known.kind === 'no_recap_yet' ? 'no recap yet' : bounded(r.known.text, r.known.truncated);
}

export function detail(e, nowMs) {
  const s = e.data.session;
  const out = e.data.final_output;
  const err = e.data.reported_error;
  return {
    id: s.id,
    title: fact(s.title),
    status: status(s.status),
    group: s.group.kind === 'project' ? fact(s.group.name) : s.group.reason,
    location: location(s.location),
    purpose: purpose(s.recap),
    configuration: `model ${fact(s.reported_configuration.model)} · effort ${fact(s.reported_configuration.effort)}`,
    snapshot: clock(e.freshness.snapshot_at, nowMs),
    age: 'known' in s.age_ms ? age(s.age_ms.known) : unknown(s.age_ms.unknown),
    finalOutput: !('known' in out)
      ? unknown(out.unknown)
      : out.known.kind === 'no_final_output_yet'
        ? 'no final output yet'
        : bounded(out.known.text, out.known.truncated),
    finalOutputTruncated: 'known' in out && out.known.kind === 'reported' && out.known.truncated,
    reportedError: !('known' in err) ? unknown(err.unknown) : err.known === null ? 'none' : bounded(err.known.text, err.known.truncated),
    coverage: e.coverage.state,
    gaps: e.gaps.map(gapLine),
  };
}
