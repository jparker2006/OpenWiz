//! The `openwiz` command: parses `ls`, `ls -l`, `show` and `--json`, calls
//! the core operations, and prints the results. `send`, `new` and `models`
//! have their own modules.
//!
//! Text and `--json` print the same envelope. The text follows the
//! provisional text contract at the end of `tests/proof/support.rs`. Errors go
//! to stderr as one `openwiz: ` line, with stdout left empty; the exit codes
//! are the `EXIT_*` constants, documented in [`help`].
//!
//! `send`, OpenWiz's first write, is in [`send`]; `new` (its second) and
//! `models` are in [`new_thread`]; `attachment add`, `ls` and `rm`, which
//! stage files on this machine only, are in [`attachment`].

pub mod attach;
pub mod attachment;
pub mod new_thread;
pub mod send;

use std::collections::HashSet;
use std::io::Write;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use openwiz_adapter_cloudroom::TESTED_VERSION;
use openwiz_core::attachment::{
    FILE_MAX_BYTES, FREE_DISK_FLOOR_BYTES, IMAGE_MAX_BYTES, UNSENT_TTL,
};
use openwiz_core::envelope::{Completeness, Envelope, SourceInfo};
use openwiz_core::limits::{FINAL_OUTPUT_MAX_BYTES, MAX_LIMIT, READ_TIMEOUT, RUN_BUDGET};
use openwiz_core::messaging::{MAX_TEXT_BYTES, OP_ID_MAX_LEN};
use openwiz_core::model::{
    BoundedText, Fact, FinalOutput, Grouping, Inventory, Location, Recap, Session, SessionId,
    SessionView, Timestamp,
};
use openwiz_core::new_thread::start::{FOLLOW_MAX_READS, FOLLOW_WINDOW};
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
        "openwiz: an inventory of CloudRoom sessions, read through room-cli, and two writes: send and new.
It also keeps files you choose ready on this machine (attachment); send and new attach them.

Usage:
  openwiz ls [-l] [-a] [--hidden] [--limit N] [--offset N] [--json]
  openwiz show <id> [--json]
  openwiz send <id> [<text>] [--steer] [--op-id <op id>] [--json]
               [--resume-paused-queue] [--accept-possible-duplicate]
               [--attach <path>]... [--attachment <id>]... [--] [<text>]
  openwiz send <id> --preview [--steer] [--json]
  openwiz send <id> --op-id <op id> --check [--json]
  openwiz new <project> --harness <h> --model <m> --effort <e> [--op-id <op id>]
              [--text-sha256 <hex>] [--attach <path>]... [--attachment <id>]...
              [--yes] [--json] [--] [<text>]
  openwiz new --project-id <id> --harness <h> --model <m> --effort <e> [...] [--] [<text>]
  openwiz new --status <op id> [--json]
  openwiz models [--json]
  openwiz attachment add [--json] [--] <path>...
  openwiz attachment ls [--json]
  openwiz attachment rm [--json] <id>...
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

send <id> <text>
  Sends one text message to one target: one existing session, one target per call
  (a shell loop calls it once per target). A file path in the text is sent as text,
  never attached. The text is the argument after the id (put -- before
  text that starts with -), or else read from stdin, byte for byte; it must not be
  empty or only whitespace, contain no NUL, and be at most {text_cap} bytes of UTF-8.
  --steer     ask to steer the current turn; the default is queue. The mode is always
              passed to room-cli explicitly, never auto, and never retried in another mode
  --op-id     the operation id (1 to {op_max} ASCII letters, digits, _ or -); one op id
              makes at most one call, ever, for one target, mode and text. Without it a
              new one is made and shown on stderr before anything is sent
  --json      print the openwiz.messaging.v1 report instead of text
  --resume-paused-queue
              send even though the target's queue appears paused (or that cannot be
              ruled out); sending resumes it, and the queued messages may run after this one
  --accept-possible-duplicate
              send even though an earlier send of this text to this target is uncertain,
              or one in the same mode is still in progress under another op id
  --attach <path>
              attach this file: it is staged first, as attachment add does (repeatable)
  --attachment <id>
              attach a file staged before, by its id (repeatable). Both keep their order,
              which the op id binds; room-cli gets the files first, then the images.
              Every attachment must be ready: if any path fails, or any attachment is
              missing, expired, changed or in use by another send or new, nothing is read
              or sent, and each one is named. Queue only: Steer can't carry attachments.
              After a queued or sent outcome is recorded, the staged copies are deleted
              (CloudRoom keeps its own); after a failure they are kept until they expire;
              while uncertain, they are kept. Delivery to the agent's machine is never
              reported
  --preview   read the target and show what a send in this mode would do: its effects
              and the read's time, or why it would be refused. Takes no text; makes
              no claim and no call, and never opens the send record (2 or 3 reads).
              --json prints openwiz.send-preview.v1
  --check     show the send record of --op-id for this target: its outcome, in progress,
              or no record. Takes no text; makes no claim and no call, and never runs
              room-cli. --json prints openwiz.send-lookup.v1

  The report keeps the requested mode apart from the reported delivery: queued,
  sent (the provider accepted the message, not that the work completed), failed
  or uncertain. Effects are worked out from one read of the target just before
  sending, and shown with its age. They can change between the read and the dispatch,
  so the outcome is what the provider reported. An uncertain send keeps its text in
  the send record and is never resent by itself. The send record is kept in the
  owner's state directory, readable by the owner only.

new <project> --harness <h> --model <m> --effort <e> <text>
  Creates and starts one Cloud thread in one existing project: a standard project
  with a GitHub remote, named as the inventory shows it (name, or name~<last 6 of its
  id>), or by --project-id. One project per call. The harness, model and effort are
  all required and passed as given: nothing is defaulted or substituted, and a choice
  CloudRoom does not offer right now (openwiz models) is refused. The text is the
  argument after the project (put -- before text that starts with -), or else read
  from stdin, as for send: at most {text_cap} bytes of UTF-8. --attach and --attachment
  attach files to the first prompt, as for send (without --yes, --attach stages the file
  on this machine only, and the printed command attaches it by --attachment).
  --yes       create it. Without --yes, new reads what a create would read, prints the
              effects and the exact command to run (the same op id, settings and text;
              every value quoted for sh, bash and zsh, in $'...' when it holds
              characters that are not safe to show), and creates nothing
  --op-id     the operation id; one op id makes at most one create, ever, for one
              project, settings and text. Without it a new one is made and shown
  --text-sha256
              create only if the text's SHA-256 is this hex; the printed command passes
              it, so a text a shell or a paste changed is refused and nothing is created
  --json      print the openwiz.new-thread.v1 report (openwiz.new-thread-preview.v1
              without --yes) instead of text
  --status    show the new-thread record of an op id: its outcome and, once created, its
              start stage from one read. Takes nothing else; makes no claim and never
              creates. --json prints openwiz.new-thread-status.v1

  Created means CloudRoom recorded the thread and its first prompt, never that it started:
  after a create, new reads the start stage at most {follow_reads} times over {follow_secs} seconds
  and shows the last. The requested configuration is shown apart from the configuration
  CloudRoom reports. An uncertain create keeps its text in the record and is never sent
  again: there is no retry. Resolve it in CloudRoom; a new create needs a new op id.

models
  The Cloud harnesses, models and efforts CloudRoom offers right now, from one cloud status
  read, as reported (it may answer from a saved copy); harnesses it lists but OpenWiz
  cannot offer, with the reason. Nothing is marked default. --json prints
  openwiz.cloud-options.v1.

attachment add <path>...
  Copies each file into this machine's private attachment store, in the owner's state
  directory (readable by the owner only), and verifies the copy: its size, SHA-256, kind
  and safe name. Ready means only that this machine holds that verified copy. Each source
  is opened read-only and never changed; a link is followed only to a regular file, and
  folders aren't supported yet. The kind comes from the bytes: PNG, JPEG, GIF and WebP are
  images; HEIC and HEIF are refused, never converted; anything else is a file, kept
  unchanged (a ZIP is never unpacked). A name whose extension doesn't match the content is
  refused, never renamed. Only CloudRoom's limits apply: {image_mib} MiB per image, and per
  file whose extension CloudRoom types as an image; {file_mib} MiB per other file; no count.
  Staging never leaves less than {floor_gib} GiB free on the store's volume. Every path is
  reported, in order, ready with its id or failed with its reason.
attachment ls
  The ready attachments, with their age and when they expire: one that no send uses is
  deleted {ttl_hours} hours after it became ready. Whenever the store is opened, copies left
  unfinished are deleted, and expired ones are deleted and reported once.
attachment rm <id>...
  Deletes each attachment's copy and metadata, unless an operation may still use it.
  attachment never sends or uploads anything, and never runs room-cli. A file path in a
  send's text is sent as text, never attached; send and new attach with --attach and
  --attachment. --json prints openwiz.attachment-add.v1,
  openwiz.attachment-list.v1 or openwiz.attachment-remove.v1.

Exit codes:
  0  a result was printed, complete or partial
  0  send: queued, or sent (the provider accepted the message, not that the work completed)
  1  a read failed
  2  usage error, or a request out of bounds
  3  show: the session was not found
  3  send: the target was not found; nothing was sent
  5  send: failed; nothing was delivered (rejected, or refused before sending; or an
     --attach path failed, and every path is reported as attachment add would)
  6  send: uncertain; the message may or may not have been accepted
  7  send: in progress in another process with this op id; no call was made
  8  send: the op id is bound to a different target, mode or text; no call was made
  9  send: possible duplicate of an uncertain or in-progress send; nothing was sent
  10 send: the send record could not be used; nothing was sent
  11 send --check: no record of this op id for this target; nothing was sent with it
  0  send --preview: a send would go ahead to its one call
  3  send --preview: the target was not found
  5  send --preview: a send would be refused, or needs --resume-paused-queue
  send --check exits as the record's outcome does (0, 3, 5, 6, 7), or 8 when the op id
  is bound to another target
  0  new: created (CloudRoom recorded it and its first prompt; never that it started)
  5  new: refused or failed; nothing was created (or an --attach path failed)
  6  new: uncertain; the thread may or may not have been created
  7  new: in progress in another process with this op id; no call was made
  8  new: the op id is bound to a different project, settings or text; no call was made
  10 new: the new-thread record could not be used; nothing was created
  11 new --status: no record of this op id; nothing was created with it
  12 new without --yes: the effects and the command were printed; nothing was created
  new --status exits as the record's outcome does (0, 5, 6, 7)
  0  models: options were printed
  13 models: nothing can be offered right now; the report says why
  0  attachment: every path ready, every id removed or expired, or the list printed
  3  attachment rm: an id was not found; the others were removed
  5  attachment add: a path failed; every path is reported, and the others are ready
  10 attachment: the attachment store could not be used; a path or id it failed on is reported
  14 attachment rm: refused while its operation is dispatching or uncertain; it is kept
On an error, stdout is empty and stderr has one line starting with \"openwiz: \".
For send, an error is a usage error (2) or an unusable send record (10); otherwise
send prints its report on stdout for every outcome (0, 3 to 9, 11), and a new op id's
line on stderr before sending. new and models do the same: a usage error (2) or an
unusable record (10) on stderr, and their report on stdout for every other exit code.
attachment does the same: a usage error (2), or a store that can't be opened (10), on stderr.
",
        default = ops::ListRequest::default().limit,
        kib = FINAL_OUTPUT_MAX_BYTES / 1024,
        read = READ_TIMEOUT.as_secs(),
        budget = RUN_BUDGET.as_secs(),
        text_cap = MAX_TEXT_BYTES,
        op_max = OP_ID_MAX_LEN,
        follow_reads = FOLLOW_MAX_READS,
        follow_secs = FOLLOW_WINDOW.as_secs(),
        image_mib = IMAGE_MAX_BYTES / (1024 * 1024),
        file_mib = FILE_MAX_BYTES / (1024 * 1024),
        floor_gib = FREE_DISK_FLOOR_BYTES / (1024 * 1024 * 1024),
        ttl_hours = UNSENT_TTL.as_secs() / 3600,
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
    format!(
        "openwiz {command} · {} · snapshot {} · {state}\n",
        source_text(&envelope.source),
        utc(envelope.freshness.snapshot_at)
    )
}

/// `<source> <version> (tested)`, or `(untested; live-tested: <version>)`:
/// how `ls`, `show` and `send` report the version.
pub(crate) fn source_text(source: &SourceInfo) -> String {
    let tested = if source.version_tested {
        "tested".to_owned()
    } else {
        format!("untested; live-tested: {}", clean(&source.tested_version))
    };
    format!(
        "{} {} ({tested})",
        clean(&source.name),
        fact(&source.version)
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
