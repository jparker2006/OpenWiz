//! The `openwiz` command: parses `ls`, `ls -l`, `show` and `--json`, calls
//! the core operations, and prints the results.
//!
//! Text and `--json` print the same envelope. The text follows the
//! provisional text contract at the end of `tests/proof/support.rs`. Errors go
//! to stderr as one `openwiz: ` line, with stdout left empty; the exit codes
//! are the `EXIT_*` constants, documented in [`help`].

use std::collections::HashSet;
use std::io::Write;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use openwiz_adapter_cloudroom::TESTED_VERSION;
use openwiz_core::envelope::{Completeness, Envelope};
use openwiz_core::limits::{FINAL_OUTPUT_MAX_BYTES, MAX_LIMIT, READ_TIMEOUT, RUN_BUDGET};
use openwiz_core::model::{
    BoundedText, Fact, FinalOutput, Grouping, Inventory, Location, Recap, Session, SessionId,
    SessionView, Timestamp,
};
use openwiz_core::ops::{self, Clock, Detail, ListRequest, OpError, Report, ShowRequest};
use openwiz_core::source::{Read, SessionSource};
use serde::Serialize;

/// A result was printed, complete or partial.
pub const EXIT_OK: i32 = 0;
/// A read failed: the session list, or the session `show` asked for.
pub const EXIT_READ_FAILED: i32 = 1;
/// A usage error, or a request out of bounds. Nothing is read.
pub const EXIT_USAGE: i32 = 2;
/// `show`: the session was not found. Nothing more is read.
pub const EXIT_NOT_FOUND: i32 = 3;

/// A parsed command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// `openwiz ls [-l] [-a] [--hidden] [--limit N] [--offset N] [--json]`
    Ls(LsArgs),
    /// `openwiz show <id> [--json]`
    Show(ShowArgs),
    /// `openwiz --help`
    Help,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LsArgs {
    /// `-l`
    pub long: bool,
    /// `-a`: include archived sessions.
    pub all: bool,
    /// `--hidden`: include hidden sessions.
    pub hidden: bool,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub json: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShowArgs {
    pub id: String,
    pub json: bool,
}

/// A command line that could not be parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsageError(pub String);

/// The wall clock and monotonic clock of this machine.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let since = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        Timestamp(i64::try_from(since.as_millis()).unwrap_or(i64::MAX))
    }

    fn monotonic(&self) -> Instant {
        Instant::now()
    }
}

/// `openwiz --help`.
pub fn help() -> String {
    format!(
        "openwiz: a read-only inventory of CloudRoom sessions, read through room-cli.

Usage:
  openwiz ls [-l] [-a] [--hidden] [--limit N] [--offset N] [--json]
  openwiz show <id> [--json]
  openwiz --help

  ls          sessions, grouped under /factories/<project>/
  -l          also read each session on the page with thread show, for its
              reported configuration (model and effort)
  -a          include archived sessions
  --hidden    include hidden sessions
  --limit N   sessions per page, 0 to {MAX_LIMIT} (default {default})
  --offset N  skip the first N sessions (default 0)
  --json      print the openwiz.inventory.v1 envelope instead of text
  show <id>   one session, with its final output (at most {kib} KiB), never a log tail

Order: most recently updated first, then by session id; sessions whose update time is unknown come last.

Every run reads room-cli --version and shows it. Any version runs; one other than the live-tested {TESTED_VERSION} is marked untested.
Each read stops after {read} seconds and is not retried; a run stops after {budget} seconds.
A result is complete or partial: its gaps say what is missing.

Exit codes:
  0  a result was printed, complete or partial
  1  a read failed
  2  usage error, or a request out of bounds
  3  show: the session was not found
On an error, stdout is empty and stderr has one line starting with \"openwiz: \".
",
        default = ops::ListRequest::default().limit,
        kib = FINAL_OUTPUT_MAX_BYTES / 1024,
        read = READ_TIMEOUT.as_secs(),
        budget = RUN_BUDGET.as_secs(),
    )
}

/// Parses the arguments after the program name.
pub fn parse(args: &[String]) -> Result<Command, UsageError> {
    let (command, rest) = match args.split_first() {
        Some((first, rest)) => (first.as_str(), rest),
        None => return Err(UsageError("no command given".to_owned())),
    };
    if rest.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(Command::Help);
    }
    match command {
        "--help" | "-h" => Ok(Command::Help),
        "ls" => parse_ls(rest).map(Command::Ls),
        "show" => parse_show(rest).map(Command::Show),
        other => Err(UsageError(format!("unknown command {}", clean(other)))),
    }
}

fn parse_ls(args: &[String]) -> Result<LsArgs, UsageError> {
    let mut ls = LsArgs::default();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => (flag, Some(value.to_owned())),
            _ => (arg.as_str(), None),
        };
        match flag {
            "--hidden" | "--json" if inline.is_some() => {
                return Err(UsageError(format!("{flag} takes no value")));
            }
            "--hidden" => ls.hidden = true,
            "--json" => ls.json = true,
            "--limit" | "--offset" => {
                let value = match inline.or_else(|| args.next().cloned()) {
                    Some(value) => value,
                    None => return Err(UsageError(format!("{flag} needs a number"))),
                };
                let n: usize = value.parse().map_err(|_| {
                    UsageError(format!(
                        "{flag} needs a whole number, not {}",
                        clean(&value)
                    ))
                })?;
                if flag == "--limit" {
                    ls.limit = Some(n);
                } else {
                    ls.offset = Some(n);
                }
            }
            short
                if short.len() > 1
                    && short.starts_with('-')
                    && !short.starts_with("--")
                    && short[1..].chars().all(|c| c == 'l' || c == 'a') =>
            {
                ls.long |= short.contains('l');
                ls.all |= short.contains('a');
            }
            other => return Err(UsageError(format!("unknown option {}", clean(other)))),
        }
    }
    Ok(ls)
}

fn parse_show(args: &[String]) -> Result<ShowArgs, UsageError> {
    let mut id = None;
    let mut json = false;
    for arg in args {
        if arg == "--json" {
            json = true;
        } else if arg.starts_with('-') {
            return Err(UsageError(format!("unknown option {}", clean(arg))));
        } else if id.is_some() {
            return Err(UsageError("show takes one session id".to_owned()));
        } else {
            id = Some(arg.clone());
        }
    }
    match id {
        Some(id) => Ok(ShowArgs { id, json }),
        None => Err(UsageError("show needs a session id".to_owned())),
    }
}

/// Runs one command and returns the process exit code.
pub fn run(
    args: &[String],
    source: &dyn SessionSource,
    clock: &dyn Clock,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    let command = match parse(args) {
        Ok(command) => command,
        Err(UsageError(message)) => {
            let _ = writeln!(stderr, "openwiz: {message} (see openwiz --help)");
            return EXIT_USAGE;
        }
    };
    let printed = match command {
        Command::Help => Ok(help()),
        Command::Ls(ls) => {
            let request = ListRequest {
                detail: if ls.long { Detail::Long } else { Detail::Short },
                include_hidden: ls.hidden,
                include_archived: ls.all,
                limit: ls.limit.unwrap_or(ListRequest::default().limit),
                offset: ls.offset.unwrap_or(0),
                budget: RUN_BUDGET,
            };
            ops::list_report(source, clock, &request).map(|report| {
                if ls.json {
                    json(&report.envelope)
                } else {
                    render_list(&report, ls.long)
                }
            })
        }
        Command::Show(show) => {
            let request = ShowRequest {
                id: SessionId(show.id),
                budget: RUN_BUDGET,
            };
            ops::show_report(source, clock, &request).map(|report| {
                if show.json {
                    json(&report.envelope)
                } else {
                    render_show(&report)
                }
            })
        }
    };
    match printed {
        Ok(text) => {
            let _ = stdout.write_all(text.as_bytes());
            let _ = stdout.flush();
            EXIT_OK
        }
        Err(error) => {
            let (code, message) = failure(source, &error);
            let _ = writeln!(stderr, "openwiz: {}", clean(&message));
            code
        }
    }
}

/// The exit code and message for an operation error.
fn failure(source: &dyn SessionSource, error: &OpError) -> (i32, String) {
    match error {
        OpError::ListFailed(e) => (
            EXIT_READ_FAILED,
            format!("{} {e}", source.read_name(Read::Sessions)),
        ),
        OpError::ShowFailed(e) => (
            EXIT_READ_FAILED,
            format!("{} {e}", source.read_name(Read::SessionDetail)),
        ),
        OpError::NotFound(id) => (EXIT_NOT_FOUND, format!("session {} not found", id.0)),
        OpError::InvalidRequest(why) => (EXIT_USAGE, why.clone()),
    }
}

fn json<T: Serialize>(envelope: &Envelope<T>) -> String {
    let mut text = serde_json::to_string_pretty(envelope).unwrap_or_else(|e| {
        // Every type here serializes; this is never expected.
        format!("{{\"error\": \"{e}\"}}")
    });
    text.push('\n');
    text
}

/// The text for `ls` (`long == false`) or `ls -l`.
pub fn render_list(report: &Report<Inventory>, long: bool) -> String {
    let envelope = &report.envelope;
    let mut out = header(if long { "ls -l" } else { "ls" }, envelope);
    if let Some(c) = &envelope.coverage.counts {
        out.push_str(&format!(
            "counts: listed {}, unreadable {}, excluded_hidden {}, excluded_archived {}, \
             hidden_read {}, archived_read {}, matched {}, offset {}, limit {}, shown {}\n",
            c.listed,
            c.unreadable,
            c.excluded_hidden,
            c.excluded_archived,
            c.hidden_read,
            c.archived_read,
            c.matched,
            c.offset,
            c.limit,
            c.shown
        ));
    }
    push_gaps(&mut out, envelope);
    // Groups by identity (the verified project id, or ungrouped), in the
    // order their first session appears; the page's order is kept inside
    // each group.
    let mut groups: Vec<(GroupKey, &Grouping, Vec<&Session>)> = Vec::new();
    for session in &envelope.data.sessions {
        let key = GroupKey::of(&session.group);
        match groups.iter_mut().find(|(k, ..)| *k == key) {
            Some((.., rows)) => rows.push(session),
            None => groups.push((key, &session.group, vec![session])),
        }
    }
    let paths = group_paths(report, groups.iter().map(|(_, group, _)| *group));
    for ((_, _, rows), path) in groups.into_iter().zip(paths) {
        out.push_str(&format!("{path}\n"));
        for session in rows {
            out.push_str(&format!("  {}", row(session)));
            if long {
                out.push_str(&format!(
                    " · provider {} · {} · {}",
                    fact(&session.provider),
                    configuration(session),
                    recap(&session.recap)
                ));
            }
            out.push('\n');
        }
    }
    if envelope.data.sessions.is_empty() {
        out.push_str("no sessions on this page\n");
    }
    out
}

/// The text for `show`.
pub fn render_show(report: &Report<SessionView>) -> String {
    let envelope = &report.envelope;
    let view = &envelope.data;
    let s = &view.session;
    let mut out = header("show", envelope);
    push_gaps(&mut out, envelope);
    let path = group_paths(report, [&s.group]).remove(0);
    out.push_str(&format!("{path}\n"));
    out.push_str(&format!("  {}\n", row(s)));
    out.push_str(&format!("  provider: {}\n", fact(&s.provider)));
    out.push_str(&format!(
        "  hidden: {} · archived: {}\n",
        yes_no(&s.hidden),
        yes_no(&s.archived)
    ));
    out.push_str(&format!("  {}\n", configuration(s)));
    out.push_str(&format!("  {}\n", recap(&s.recap)));
    out.push_str(&format!(
        "  reported error: {}\n",
        match &view.reported_error {
            Fact::Known(Some(error)) => bounded(error),
            Fact::Known(None) => "none".to_owned(),
            Fact::Unknown(why) => unknown(why),
        }
    ));
    match &view.final_output {
        Fact::Known(FinalOutput::Reported(output)) => {
            out.push_str("final output:\n");
            out.push_str(&clean_block(&output.text));
            if output.truncated {
                out.push('…');
            }
            out.push('\n');
            if output.truncated {
                out.push_str(&format!(
                    "final output cut at {FINAL_OUTPUT_MAX_BYTES} bytes\n"
                ));
            }
        }
        Fact::Known(FinalOutput::NoFinalOutputYet) => {
            out.push_str("final output: no final output yet\n");
        }
        Fact::Unknown(why) => out.push_str(&format!("final output: {}\n", unknown(why))),
    }
    out
}

/// Line 1: the source and its version, the snapshot time and the coverage.
fn header<T>(command: &str, envelope: &Envelope<T>) -> String {
    let state = match envelope.coverage.state {
        Completeness::Complete => "complete".to_owned(),
        Completeness::Partial => format!("partial, {} gaps", envelope.gaps.len()),
    };
    let tested = if envelope.source.version_tested {
        "tested".to_owned()
    } else {
        format!(
            "untested; live-tested: {}",
            clean(&envelope.source.tested_version)
        )
    };
    format!(
        "openwiz {command} · {} {} ({tested}) · snapshot {} · {state}\n",
        clean(&envelope.source.name),
        fact(&envelope.source.version),
        utc(envelope.freshness.snapshot_at)
    )
}

fn push_gaps<T>(out: &mut String, envelope: &Envelope<T>) {
    for gap in &envelope.gaps {
        let kind = serde_json::to_value(gap.kind)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        out.push_str(&format!("gap: {kind}"));
        if let Some(subject) = &gap.subject {
            out.push_str(&format!(" · {}", clean(subject)));
        }
        out.push_str(&format!(" · {}\n", clean(&gap.detail)));
    }
}

/// The items every session line shows: id, status, age, location, parent,
/// flags, title, and the reason when it is ungrouped.
fn row(s: &Session) -> String {
    let parent = match &s.parent {
        Fact::Known(None) => "none".to_owned(),
        Fact::Known(Some(id)) => clean(&id.0),
        Fact::Unknown(why) => unknown(why),
    };
    let location = match &s.location {
        Fact::Known(Location::Cloud) => "cloud".to_owned(),
        Fact::Known(Location::Local { machine }) => format!("local on {}", fact(machine)),
        Fact::Unknown(why) => unknown(why),
    };
    let age = match &s.age_ms {
        Fact::Known(ms) => age(*ms),
        Fact::Unknown(why) => unknown(why),
    };
    let mut line = format!(
        "{} · status {} · age {age} · location {location} · parent {parent}",
        clean(&s.id.0),
        fact(&s.status)
    );
    for (flag, value) in [("hidden", &s.hidden), ("archived", &s.archived)] {
        match value {
            Fact::Known(true) => line.push_str(&format!(" · {flag}")),
            Fact::Known(false) => {}
            Fact::Unknown(why) => line.push_str(&format!(" · {flag} {}", unknown(why))),
        }
    }
    line.push_str(&format!(" · title {}", fact(&s.title)));
    if let Grouping::Ungrouped { reason } = &s.group {
        line.push_str(&format!(" · ungrouped: {}", clean(reason)));
    }
    line
}

fn configuration(s: &Session) -> String {
    format!(
        "reported configuration: model {}, effort {}",
        fact(&s.reported_configuration.model),
        fact(&s.reported_configuration.effort)
    )
}

fn recap(recap: &Fact<Recap>) -> String {
    match recap {
        Fact::Known(Recap::PurposeAsReported(text)) => {
            format!("purpose, as reported: {}", bounded(text))
        }
        Fact::Known(Recap::NoRecapYet) => "no recap yet".to_owned(),
        Fact::Unknown(why) => format!("recap {}", unknown(why)),
    }
}

/// A group's identity in the text: its verified project id, or ungrouped.
#[derive(Clone, Debug, PartialEq, Eq)]
enum GroupKey {
    Project(String),
    Ungrouped,
}

impl GroupKey {
    fn of(group: &Grouping) -> Self {
        match group {
            Grouping::Project { id, .. } => Self::Project(id.0.clone()),
            Grouping::Ungrouped { .. } => Self::Ungrouped,
        }
    }
}

const UNGROUPED: &str = "_ungrouped";

/// The header path of each group, one per group and all different:
/// `/factories/_ungrouped/`, or `/factories/<name>/`, with
/// `~<last 6 characters of the id>` when the run's project list shares the
/// name, when the name is `_ungrouped`, or when two groups here would
/// otherwise print the same; the whole id if even that is not enough; and
/// a last `~<n>` for anything still printed twice.
fn group_paths<'g, T>(
    report: &Report<T>,
    groups: impl IntoIterator<Item = &'g Grouping>,
) -> Vec<String> {
    let groups: Vec<&Grouping> = groups.into_iter().collect();
    let base = |group: &Grouping| match group {
        Grouping::Project { name, .. } => fact(name),
        Grouping::Ungrouped { .. } => UNGROUPED.to_owned(),
    };
    let mut labels: Vec<String> = groups
        .iter()
        .map(|group| match group {
            Grouping::Project { id, name } => {
                let name = fact(name);
                if report.shared_names.contains(id) || name == UNGROUPED {
                    format!("{name}~{}", last_six(&id.0))
                } else {
                    name
                }
            }
            Grouping::Ungrouped { .. } => UNGROUPED.to_owned(),
        })
        .collect();
    for widen in [
        |name: &str, id: &str| format!("{name}~{}", last_six(id)),
        |name: &str, id: &str| format!("{name}~{}", clean(id)),
    ] {
        let clashing: Vec<bool> = labels
            .iter()
            .map(|label| labels.iter().filter(|other| *other == label).count() > 1)
            .collect();
        for (i, group) in groups.iter().enumerate() {
            if let (true, Grouping::Project { id, .. }) = (clashing[i], group) {
                labels[i] = widen(&base(group), &id.0);
            }
        }
    }
    // Anything still printed twice (a name can copy another group's widened
    // header) keeps its first use; each later one gets the lowest `~<n>`
    // that is free.
    let mut used: HashSet<String> = HashSet::new();
    for (i, group) in groups.iter().enumerate() {
        if matches!(group, Grouping::Ungrouped { .. }) {
            used.insert(labels[i].clone());
        }
    }
    for (i, group) in groups.iter().enumerate() {
        if matches!(group, Grouping::Ungrouped { .. }) || used.insert(labels[i].clone()) {
            continue;
        }
        let free = (2..)
            .map(|n| format!("{}~{n}", labels[i]))
            .find(|label| !used.contains(label) && !labels.contains(label))
            .expect("an unused number exists");
        used.insert(free.clone());
        labels[i] = free;
    }
    labels
        .into_iter()
        .map(|label| format!("/factories/{label}/"))
        .collect()
}

fn last_six(id: &str) -> String {
    let chars: Vec<char> = id.chars().collect();
    clean(
        &chars[chars.len().saturating_sub(6)..]
            .iter()
            .collect::<String>(),
    )
}

fn yes_no(value: &Fact<bool>) -> String {
    match value {
        Fact::Known(true) => "yes".to_owned(),
        Fact::Known(false) => "no".to_owned(),
        Fact::Unknown(why) => unknown(why),
    }
}

fn fact(value: &Fact<String>) -> String {
    match value {
        Fact::Known(text) => clean(text),
        Fact::Unknown(why) => unknown(why),
    }
}

fn unknown(why: &str) -> String {
    format!("unknown ({})", clean(why))
}

/// A cut text is shown with `…` right after what was kept.
fn bounded(text: &BoundedText) -> String {
    let mut shown = clean(&text.text);
    if text.truncated {
        shown.push('…');
    }
    shown
}

/// Characters that could break a line, drive the terminal or reorder what
/// it shows: controls, line and paragraph separators, bidi controls and
/// zero-width or invisible format marks.
fn unsafe_for_terminal(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{00AD}'
                | '\u{061C}'
                | '\u{180E}'
                | '\u{200B}'..='\u{200F}'
                | '\u{2028}'..='\u{202E}'
                | '\u{2060}'..='\u{206F}'
                | '\u{FEFF}'
                | '\u{FFF9}'..='\u{FFFB}'
        )
}

/// What an unsafe character is shown as: `�`, or `?` where `�` would take
/// more bytes than the character did, so a cut text never grows.
fn replacement(c: char) -> char {
    if c.len_utf8() >= '\u{FFFD}'.len_utf8() {
        '\u{FFFD}'
    } else {
        '?'
    }
}

/// One-line text: every character unsafe for the terminal is replaced.
fn clean(text: &str) -> String {
    text.chars()
        .map(|c| {
            if unsafe_for_terminal(c) {
                replacement(c)
            } else {
                c
            }
        })
        .collect()
}

/// A block of text: as `clean`, but line breaks and tabs are kept.
fn clean_block(text: &str) -> String {
    text.chars()
        .map(|c| {
            if unsafe_for_terminal(c) && c != '\n' && c != '\t' {
                replacement(c)
            } else {
                c
            }
        })
        .collect()
}

/// A known age: the whole number of the largest unit it reaches.
fn age(ms: i64) -> String {
    let sign = if ms < 0 { "-" } else { "" };
    let s = ms.unsigned_abs() / 1_000;
    for (unit, size) in [("d", 86_400), ("h", 3_600), ("m", 60)] {
        if s >= size {
            return format!("{sign}{}{unit}", s / size);
        }
    }
    format!("{sign}{s}s")
}

/// Milliseconds since the Unix epoch as RFC 3339 UTC, whole seconds.
fn utc(at: Timestamp) -> String {
    let secs = at.0.div_euclid(1_000);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Days to a civil date (H. Hinnant's algorithm).
    let z = days + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}
