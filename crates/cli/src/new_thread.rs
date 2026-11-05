//! `openwiz new`, `openwiz new --status` and `openwiz models` (OpenWiz
//! 0.0.4, the implementation): parse the command line, call the core's
//! `new_thread`, `preview_new_thread`, `new_thread_status` and
//! `cloud_options`, and print the result as text or as the
//! `contracts/new-thread.v1.schema.json` JSON.
//!
//! Text and `--json` print the same report. Usage errors and an unusable
//! record go to stderr as one `openwiz: ` line, with stdout empty. The exit
//! codes are the `EXIT_*` constants here, documented in
//! [`help`](crate::help).
//!
//! **`new` creates only with `--yes`** (sheet the check). Without it, it reads
//! what a create would read, prints the effects and the exact command, and
//! exits [`EXIT_NEEDS_YES`]: nothing is claimed or created. There is no
//! retry (sheet the check): an uncertain create stays uncertain, and the owner
//! resolves it in CloudRoom.

use std::io::{Read, Write};
use std::time::Duration;

use openwiz_core::limits::READ_TIMEOUT;
use openwiz_core::messaging::record::StoreError;
use openwiz_core::messaging::{InvalidRequest, MAX_TEXT_BYTES, OpId, sha256_hex};
use openwiz_core::model::Fact;
use openwiz_core::new_thread::effects::Effects;
use openwiz_core::new_thread::operation::{
    NewThreadClient, NewThreadError, NewThreadPreview, NewThreadReport, NewThreadResult,
    PreviewVerdict, new_thread_with, preview_new_thread_with,
};
use openwiz_core::new_thread::options::{CloudOptions, OptionsState, cloud_options};
use openwiz_core::new_thread::outcome::{Gap, Outcome};
use openwiz_core::new_thread::record::NewThreadStore;
use openwiz_core::new_thread::start::{StartStage, StartView, follow_start};
use openwiz_core::new_thread::status::{NewThreadStatus, StatusResult, new_thread_status};
use openwiz_core::new_thread::{
    NEW_THREAD_BUDGET, NewThreadRequest, ProjectRef, ProjectView, STATUS_SCHEMA_ID, Settings,
};
use openwiz_core::ops::Clock;
use openwiz_core::prompt::{NoAttachments, PromptPayload, StagedAttachments};
use openwiz_store::attachments::Df;
use serde::Serialize;

use crate::attach::{self, AttachArg, Prepared};
use crate::send::{OpenAttachments, no_attachment_store};
use crate::{EXIT_OK, EXIT_USAGE, UsageError, clean, help, source_text, unsafe_for_terminal, utc};

/// `new`: created. CloudRoom recorded the thread and its first prompt; not
/// that it started (the start stage is shown apart). `new --status` of a
/// created thread, and `models` with something offered, exit 0 too.
pub const EXIT_CREATED: i32 = 0;
/// `new`: refused or failed. Nothing was created.
pub const EXIT_NOT_CREATED: i32 = 5;
/// `new`: uncertain. The thread may or may not have been created; the
/// prompt is kept in the record, and nothing is retried.
pub const EXIT_NEW_UNCERTAIN: i32 = 6;
/// `new`: another process is creating with this op id. No call was made.
pub const EXIT_NEW_IN_PROGRESS: i32 = 7;
/// `new`: this op id is bound to a different project, setting or text. No
/// call was made.
pub const EXIT_NEW_CONFLICTING_REUSE: i32 = 8;
/// `new`: the new-thread record could not be opened, read or written.
/// Nothing was created.
pub const EXIT_NEW_RECORD_UNAVAILABLE: i32 = 10;
/// `new --status`: no record has this op id. Nothing was created with it
/// from this record.
pub const EXIT_NEW_NO_RECORD: i32 = 11;
/// `new` without `--yes`: the effects and the exact command were printed,
/// and nothing was claimed or created.
pub const EXIT_NEEDS_YES: i32 = 12;
/// `models`: nothing can be offered right now; the report says why.
pub const EXIT_NO_OPTIONS: i32 = 13;

/// Creates the thread; without it, `new` only shows what a create would do.
pub const YES: &str = "--yes";
/// Shows the record of an op id; no claim, no call.
pub const STATUS: &str = "--status";
/// Refuses a text whose SHA-256 is not this one .
pub const TEXT_SHA256: &str = "--text-sha256";

/// What `openwiz new` needs from the program around it.
pub struct NewContext<'a> {
    pub client: &'a dyn NewThreadClient,
    /// Opens the new-thread record. Called only for a create (`--yes`), once
    /// the command line is valid.
    pub open_store: &'a dyn Fn() -> Result<Box<dyn NewThreadStore>, StoreError>,
    /// Opens the record for reading only (`--status`): creates nothing, and
    /// `None` when it does not exist yet.
    pub open_store_read_only: &'a dyn Fn() -> Result<Option<Box<dyn NewThreadStore>>, StoreError>,
    pub clock: &'a dyn Clock,
    /// Where the text comes from when it is not on the command line.
    pub stdin: &'a mut dyn Read,
    /// A terminal is never read for the text: that would wait for typing.
    pub stdin_is_terminal: bool,
    /// Makes a new op id when `--op-id` is not given.
    pub new_op_id: &'a dyn Fn() -> Result<OpId, String>,
    /// Waits between two reads of a created thread's start.
    pub wait: &'a dyn Fn(Duration),
}

/// A parsed `openwiz new` command line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewArgs {
    /// The project: a name (`name` or `name~<last 6 of its id>`), or with
    /// `--project-id` its id.
    pub project: Option<ProjectRef>,
    pub harness: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// `None`: read the text from stdin.
    pub text: Option<String>,
    pub op_id: Option<String>,
    pub yes: bool,
    pub json: bool,
    /// `--status <op id>`.
    pub status: Option<String>,
    /// `--text-sha256 <hex>`: create only if the text's SHA-256 is this.
    /// The preview's command passes it, so a text changed on the way (by a
    /// shell, a paste or other input) is refused, never created.
    pub text_sha256: Option<String>,
    /// `--attach <path>` and `--attachment <id>`, in command-line order (B2).
    pub attachments: Vec<AttachArg>,
}

/// What `openwiz new` was asked to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NewCommand {
    New(Box<NewArgs>),
    Help,
}

/// Parses the arguments after `new`. Every option comes before `--`;
/// everything after it is the project name (unless `--project-id` is given)
/// and the text.
pub fn parse_new(args: &[String]) -> Result<NewCommand, UsageError> {
    let mut parsed = NewArgs::default();
    let mut project_id: Option<String> = None;
    let mut positionals: Vec<String> = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--" {
            positionals.extend(args.by_ref().cloned());
            break;
        }
        if !arg.starts_with('-') {
            positionals.push(arg.clone());
            continue;
        }
        let (flag, inline) = match arg.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => (flag, Some(value)),
            _ => (arg.as_str(), None),
        };
        let mut value = |name: &str| -> Result<String, UsageError> {
            match inline {
                Some(value) => Ok(value.to_owned()),
                None => args
                    .next()
                    .cloned()
                    .ok_or_else(|| UsageError(format!("{name} needs a value"))),
            }
        };
        if attach::is_flag(flag) {
            let given = value(flag)?;
            let parsed_arg = attach::parse_flag(flag, given).expect("an attachment flag")?;
            parsed.attachments.push(parsed_arg);
            continue;
        }
        let slot = match flag {
            "--help" | "-h" => return Ok(NewCommand::Help),
            "--project-id" => &mut project_id,
            "--harness" => &mut parsed.harness,
            "--model" => &mut parsed.model,
            "--effort" => &mut parsed.effort,
            "--op-id" => &mut parsed.op_id,
            TEXT_SHA256 => &mut parsed.text_sha256,
            STATUS => &mut parsed.status,
            YES | "--json" if inline.is_some() => {
                return Err(UsageError(format!("{flag} takes no value")));
            }
            YES => {
                parsed.yes = true;
                continue;
            }
            "--json" => {
                parsed.json = true;
                continue;
            }
            other => {
                return Err(UsageError(format!(
                    "unknown option {} (put -- before text that starts with -)",
                    clean(other)
                )));
            }
        };
        if slot.is_some() {
            return Err(UsageError(format!("{flag} is given twice")));
        }
        *slot = Some(value(flag)?);
    }
    let mut positionals = positionals.into_iter();
    if parsed.status.is_some() {
        let given = [
            (project_id.is_some(), "--project-id"),
            (parsed.harness.is_some(), "--harness"),
            (parsed.model.is_some(), "--model"),
            (parsed.effort.is_some(), "--effort"),
            (parsed.op_id.is_some(), "--op-id"),
            (parsed.yes, YES),
            (parsed.text_sha256.is_some(), TEXT_SHA256),
            (!parsed.attachments.is_empty(), "attachment"),
        ];
        if let Some((_, flag)) = given.iter().find(|(given, _)| *given) {
            return Err(UsageError(format!(
                "{STATUS} takes no {flag}: it never creates"
            )));
        }
        if positionals.next().is_some() {
            return Err(UsageError(format!(
                "{STATUS} takes no project and no text: it never creates"
            )));
        }
        return Ok(NewCommand::New(Box::new(parsed)));
    }
    parsed.project = match project_id {
        Some(id) => Some(ProjectRef::Id(id)),
        None => Some(ProjectRef::Name(positionals.next().ok_or_else(|| {
            UsageError("new needs a project (its name, or --project-id <id>)".to_owned())
        })?)),
    };
    parsed.text = positionals.next();
    if positionals.next().is_some() {
        return Err(UsageError(
            "new takes one project and one text (quote the text)".to_owned(),
        ));
    }
    for (given, flag) in [
        (&parsed.harness, "--harness"),
        (&parsed.model, "--model"),
        (&parsed.effort, "--effort"),
    ] {
        if given.is_none() {
            return Err(UsageError(format!(
                "new needs {flag}; nothing is defaulted (see openwiz models)"
            )));
        }
    }
    Ok(NewCommand::New(Box::new(parsed)))
}

fn usage(stderr: &mut dyn Write, message: &str) -> i32 {
    let _ = writeln!(
        stderr,
        "openwiz: {}; nothing was created (see openwiz --help)",
        clean(message)
    );
    EXIT_USAGE
}

fn unusable(stderr: &mut dyn Write, what: &str, why: &str) -> i32 {
    let _ = writeln!(
        stderr,
        "openwiz: the new-thread record could not be {what}: {}; nothing was created",
        clean(why)
    );
    EXIT_NEW_RECORD_UNAVAILABLE
}

/// Prints `value` as pretty JSON, or `text`.
fn print(stdout: &mut dyn Write, json: bool, value: &impl Serialize, text: String) {
    let printed = if json {
        let mut out = serde_json::to_string_pretty(value)
            .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
        out.push('\n');
        out
    } else {
        text
    };
    let _ = stdout.write_all(printed.as_bytes());
    let _ = stdout.flush();
}

/// Runs `openwiz new <args>` (the arguments after `new`) with no
/// attachment store, and returns the exit code: any `--attach` or
/// `--attachment` is refused.
pub fn run_new(
    args: &[String],
    context: NewContext<'_>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    run_new_with(args, context, &no_attachment_store, stdout, stderr)
}

/// Runs `openwiz new <args>` (the arguments after `new`), with the
/// attachment store `open_attachments` opens, and returns the exit code.
pub fn run_new_with(
    args: &[String],
    context: NewContext<'_>,
    open_attachments: OpenAttachments<'_>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    let args = match parse_new(args) {
        Ok(NewCommand::Help) => {
            let _ = stdout.write_all(help().as_bytes());
            return EXIT_OK;
        }
        Ok(NewCommand::New(args)) => args,
        Err(UsageError(message)) => return usage(stderr, &message),
    };
    if let Some(op_id) = &args.status {
        return run_status(op_id, args.json, &context, stdout, stderr);
    }
    let text = match &args.text {
        Some(text) => text.clone(),
        None if context.stdin_is_terminal => {
            return usage(
                stderr,
                "give the text after the project, or pipe it on stdin (a terminal is never read)",
            );
        }
        None => {
            let mut bytes = Vec::new();
            let limit = MAX_TEXT_BYTES as u64 + 1;
            if context.stdin.take(limit).read_to_end(&mut bytes).is_err() {
                return usage(stderr, "the text could not be read from stdin");
            }
            match String::from_utf8(bytes) {
                Ok(text) => text,
                Err(_) => return usage(stderr, "the text from stdin is not valid UTF-8"),
            }
        }
    };
    if let Some(want) = &args.text_sha256 {
        let hex = want.len() == 64 && want.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if !hex {
            return usage(stderr, "--text-sha256 takes 64 lowercase hex digits");
        }
        if sha256_hex(text.as_bytes()) != *want {
            return usage(
                stderr,
                "the text is not the previewed text: its SHA-256 is not --text-sha256 (a shell or \
                 a paste may have changed it)",
            );
        }
    }
    let given = match args.op_id.as_deref().map(OpId::parse).transpose() {
        Ok(given) => given,
        Err(InvalidRequest(why)) => return usage(stderr, &why),
    };
    let request_of = |op_id: OpId, attachments: Vec<_>| {
        let mut prompt = PromptPayload::text(text.clone());
        prompt.attachments = attachments;
        NewThreadRequest::new(
            args.project.clone().expect("parsed"),
            Settings {
                harness: args.harness.clone().unwrap_or_default(),
                model: args.model.clone().unwrap_or_default(),
                effort: args.effort.clone().unwrap_or_default(),
            },
            prompt,
            op_id,
        )
    };
    // Everything checkable without a read, before any read or record: the
    // op id only stands in here.
    if let Err(InvalidRequest(why)) =
        request_of(given.clone().unwrap_or_else(placeholder), vec![]).validate()
    {
        return usage(stderr, &why);
    }

    // B2: the attachments, staged (on this machine only) or named, before
    // any read. A path that fails stops everything: nothing is created.
    let mut ids = Vec::new();
    let mut attachment_store = None;
    if !args.attachments.is_empty() {
        let opened = match open_attachments() {
            Ok(opened) => opened,
            Err(e) => {
                let _ = writeln!(
                    stderr,
                    "openwiz: {}; nothing was created",
                    attach::store_unusable(&e)
                );
                return EXIT_NEW_RECORD_UNAVAILABLE;
            }
        };
        match attach::prepare(&args.attachments, &opened, context.clock.now(), &Df) {
            Prepared::Ids(prepared) => ids = prepared,
            Prepared::Failed(report) => {
                let text = attach::failed_text(&report, "nothing was created");
                print(stdout, args.json, &report, text);
                return match attach::failed_code(&report) {
                    EXIT_NEW_RECORD_UNAVAILABLE => EXIT_NEW_RECORD_UNAVAILABLE,
                    _ => EXIT_NOT_CREATED,
                };
            }
        }
        attachment_store = Some(opened);
    }
    let attachments: &dyn StagedAttachments = match &attachment_store {
        Some(store) => store,
        None => &NoAttachments,
    };
    let request = |op_id: OpId| request_of(op_id, ids.clone());
    if let Err(InvalidRequest(why)) = request(given.clone().unwrap_or_else(placeholder)).validate()
    {
        return usage(stderr, &why);
    }
    if !args.yes {
        let op_id = match given {
            Some(op_id) => op_id,
            None => match (context.new_op_id)() {
                Ok(op_id) => op_id,
                Err(why) => return no_op_id(stderr, &why),
            },
        };
        return run_preview(
            &args,
            &request(op_id),
            attachments,
            &context,
            stdout,
            stderr,
        );
    }
    let store = match (context.open_store)() {
        Ok(store) => store,
        Err(StoreError(why)) => return unusable(stderr, "opened", &why),
    };
    let op_id = match given {
        Some(op_id) => op_id,
        None => match (context.new_op_id)() {
            Ok(op_id) => {
                // Shown before anything is created, so a create that is cut
                // short can still be checked.
                let _ = writeln!(
                    stderr,
                    "openwiz: op id {op_id} (use openwiz new --status {op_id} to check this \
                     create)"
                );
                op_id
            }
            Err(why) => return no_op_id(stderr, &why),
        },
    };
    match new_thread_with(
        context.client,
        store.as_ref(),
        attachments,
        context.clock,
        &request(op_id),
    ) {
        Ok(mut report) => {
            if let NewThreadResult::Outcome {
                outcome: Outcome::Created { thread_id, .. },
                ..
            } = &report.result
            {
                let (start, _) = follow_start(
                    context.client,
                    context.clock,
                    thread_id,
                    requested(&report),
                    context.wait,
                );
                report.start = Some(start);
            }
            print(stdout, args.json, &report, render_report(&report));
            exit_code(&report)
        }
        Err(NewThreadError::Invalid(InvalidRequest(why))) => usage(stderr, &why),
        Err(NewThreadError::Store(StoreError(why))) => unusable(stderr, "used", &why),
    }
}

/// Stands in for the op id when the request is checked before one is made.
fn placeholder() -> OpId {
    OpId::parse("op-check").expect("a valid op id")
}

fn no_op_id(stderr: &mut dyn Write, why: &str) -> i32 {
    let _ = writeln!(
        stderr,
        "openwiz: no op id could be made: {}; nothing was created",
        clean(why)
    );
    EXIT_NEW_RECORD_UNAVAILABLE
}

/// The requested `spawn` provider (or harness, before the options were
/// checked), model and effort of `report`.
fn requested(report: &NewThreadReport) -> (&str, &str, &str) {
    (
        report
            .provider
            .as_deref()
            .unwrap_or(&report.requested.harness),
        &report.requested.model,
        &report.requested.effort,
    )
}

/// `openwiz new` without `--yes`: the reads a create makes, and what it
/// would do. No record is opened, nothing is claimed or called.
fn run_preview(
    args: &NewArgs,
    request: &NewThreadRequest,
    attachments: &dyn StagedAttachments,
    context: &NewContext<'_>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    match preview_new_thread_with(context.client, attachments, context.clock, request) {
        Ok(preview) => {
            let text = render_preview(&preview, &request.prompt.text);
            print(stdout, args.json, &preview, text);
            match &preview.verdict {
                PreviewVerdict::Ready { .. } => EXIT_NEEDS_YES,
                PreviewVerdict::Stopped { outcome } => outcome_code(outcome),
            }
        }
        Err(NewThreadError::Invalid(InvalidRequest(why))) => usage(stderr, &why),
        Err(NewThreadError::Store(StoreError(why))) => {
            let _ = writeln!(
                stderr,
                "openwiz: the attachment store could not be used: {}; nothing was created",
                clean(&why)
            );
            EXIT_NEW_RECORD_UNAVAILABLE
        }
    }
}

/// `openwiz new --status <op id>`: the record only, through the read-only
/// opener (nothing is created or written), and one read of what it names.
/// Never a `spawn`.
fn run_status(
    op_id: &str,
    json: bool,
    context: &NewContext<'_>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    let op_id = match OpId::parse(op_id) {
        Ok(op_id) => op_id,
        Err(InvalidRequest(why)) => return usage(stderr, &why),
    };
    let store = match (context.open_store_read_only)() {
        Ok(store) => store,
        Err(StoreError(why)) => return unusable(stderr, "read", &why),
    };
    let status = match store {
        Some(store) => match new_thread_status(
            context.client,
            store.as_ref(),
            context.clock,
            &op_id,
            NEW_THREAD_BUDGET,
        ) {
            Ok(status) => status,
            Err(StoreError(why)) => return unusable(stderr, "read", &why),
        },
        // No record yet anywhere: nothing to read.
        None => NewThreadStatus {
            schema: STATUS_SCHEMA_ID.to_owned(),
            op_id,
            result: StatusResult::NoRecord,
        },
    };
    print(stdout, json, &status, render_status(&status));
    match &status.result {
        StatusResult::NoRecord => EXIT_NEW_NO_RECORD,
        StatusResult::Recorded(recorded) => exit_code(&recorded.report),
    }
}

/// Runs `openwiz models <args>` and returns the exit code: one `cloud
/// status` read.
pub fn run_models(
    args: &[String],
    client: &dyn NewThreadClient,
    clock: &dyn Clock,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    let mut json = false;
    for arg in args {
        match arg.as_str() {
            "--help" | "-h" => {
                let _ = stdout.write_all(help().as_bytes());
                return EXIT_OK;
            }
            "--json" => json = true,
            other => {
                let _ = writeln!(
                    stderr,
                    "openwiz: models takes only --json, not {}; nothing was read (see openwiz \
                     --help)",
                    clean(other)
                );
                return EXIT_USAGE;
            }
        }
    }
    let deadline = clock.monotonic() + READ_TIMEOUT;
    let options = cloud_options(client.catalogue(deadline), client.cli_levels(), clock.now());
    print(stdout, json, &options, render_options(&options));
    match options.state {
        OptionsState::Offered => EXIT_CREATED,
        OptionsState::NoOptions { .. } => EXIT_NO_OPTIONS,
    }
}

fn outcome_code(outcome: &Outcome) -> i32 {
    match outcome {
        Outcome::Created { .. } => EXIT_CREATED,
        Outcome::Refused { .. } | Outcome::Failed { .. } => EXIT_NOT_CREATED,
        Outcome::Uncertain { .. } => EXIT_NEW_UNCERTAIN,
    }
}

/// The exit code of a new-thread report.
pub fn exit_code(report: &NewThreadReport) -> i32 {
    match &report.result {
        NewThreadResult::Outcome { outcome, .. } => outcome_code(outcome),
        NewThreadResult::InProgress { .. } => EXIT_NEW_IN_PROGRESS,
        NewThreadResult::ConflictingReuse { .. } => EXIT_NEW_CONFLICTING_REUSE,
    }
}

fn quoted(value: &str) -> String {
    if value.is_empty() {
        return "''".to_owned();
    }
    if !value.chars().any(|c| c != '\n' && unsafe_for_terminal(c)) {
        return value
            .split('\'')
            .map(|run| {
                if run.is_empty() {
                    String::new()
                } else {
                    format!("'{run}'")
                }
            })
            .collect::<Vec<_>>()
            .join("\\'");
    }
    let mut word = String::from("$'");
    for c in value.chars() {
        match c {
            '\\' => word.push_str("\\\\"),
            '\'' => word.push_str("\\'"),
            '\n' => word.push_str("\\n"),
            '\t' => word.push_str("\\t"),
            '\r' => word.push_str("\\r"),
            '!' => word.push_str("\\x21"),
            c if unsafe_for_terminal(c) => {
                let mut bytes = [0; 4];
                for b in c.encode_utf8(&mut bytes).bytes() {
                    word.push_str(&format!("\\x{b:02x}"));
                }
            }
            c => word.push(c),
        }
    }
    word.push('\'');
    word
}

/// The exact command that creates what `preview` shows, run as printed: the
/// same project, settings, op id and `text`, the text's SHA-256 (so a text
/// changed on the way is refused, never created), each attachment as
/// `--attachment <id>` in the owner's order (B2: a path given with
/// `--attach` was staged by the preview, so the create attaches exactly
/// that copy), and `--yes`. Only
/// OpenWiz's own words and the hash are bare; every value is [`quoted`]. The
/// text is always the last word, after `--` (so a text starting with `-`
/// stays the text), whether it came as the argument or on stdin.
fn command(preview: &NewThreadPreview, text: &str) -> String {
    // The hash is OpenWiz's own: 64 hex digits, safe bare.
    let attachments: String = preview
        .attachments
        .iter()
        .map(|a| format!(" {} {}", attach::ATTACHMENT, quoted(a.id.as_str())))
        .collect();
    let settings = format!(
        "--harness {} --model {} --effort {} --op-id {} {TEXT_SHA256} {}{attachments} --yes",
        quoted(&preview.requested.harness),
        quoted(&preview.requested.model),
        quoted(&preview.requested.effort),
        quoted(preview.op_id.as_str()),
        sha256_hex(text.as_bytes())
    );
    let text = quoted(text);
    match &preview.project_ref {
        ProjectRef::Id(id) => format!(
            "openwiz new --project-id {} {settings} -- {text}",
            quoted(id)
        ),
        ProjectRef::Name(name) if name.starts_with('-') => {
            format!("openwiz new {settings} -- {} {text}", quoted(name))
        }
        ProjectRef::Name(name) => format!("openwiz new {} {settings} -- {text}", quoted(name)),
    }
}

fn fact_text(value: &Fact<String>) -> String {
    match value {
        Fact::Known(text) => clean(text),
        Fact::Unknown(why) => format!("unknown ({})", clean(why)),
    }
}

fn project_line(project_ref: &ProjectRef, project: Option<&ProjectView>) -> String {
    match project {
        Some(view) => format!("{} · {}", clean(&view.id), fact_text(&view.name)),
        None => format!("{} (as given)", clean(project_ref.as_str())),
    }
}

fn requested_line(settings: &Settings) -> String {
    format!(
        "requested: harness {} · model {} · effort {}\n",
        clean(&settings.harness),
        clean(&settings.model),
        clean(&settings.effort)
    )
}

fn push_effects(out: &mut String, effects: &Effects, when: &str) {
    out.push_str(&format!(
        "effects ({when}, read {}):\n",
        utc(effects.read_at)
    ));
    for effect in &effects.effects {
        out.push_str(&format!("  - {}\n", clean(&effect.text)));
    }
}

fn push_gaps(out: &mut String, gaps: &[Gap]) {
    for gap in gaps {
        out.push_str(&format!(
            "gap: {} · {}\n",
            clean(&gap.subject),
            clean(&gap.detail)
        ));
    }
}

/// The text of a preview of a create of `text`: what it would do, and the
/// command that creates it, exactly. See `tests/new-thread/n18_cli.rs`.
pub fn render_preview(preview: &NewThreadPreview, text: &str) -> String {
    let mut out = format!(
        "openwiz new · preview · op {} · {}\n",
        preview.op_id,
        project_line(&preview.project_ref, preview.project.as_ref())
    );
    out.push_str(&format!("source: {}\n", source_text(&preview.source)));
    out.push_str(&requested_line(&preview.requested));
    out.push_str(&format!(
        "text: {} bytes, SHA-256 {}\n",
        preview.text_bytes, preview.text_sha256
    ));
    out.push_str(&attach::lines(&preview.attachments));
    push_gaps(&mut out, &preview.gaps);
    match &preview.verdict {
        PreviewVerdict::Ready { provider } => {
            if let Some(effects) = &preview.effects {
                push_effects(&mut out, effects, "a create would do this");
            }
            // Last, since a text with a line break spans lines. The command
            // is safe to show as it is: `quoted` escapes every character
            // that is not.
            out.push_str(&format!(
                "spawn provider: {}\nnothing was created. To create it, run this (the same op id, \
                 settings and text, and --yes):\n  {}\n",
                clean(provider),
                command(preview, text)
            ));
        }
        PreviewVerdict::Stopped { outcome } => {
            out.push_str(&format!("{}\n", clean(outcome.text())));
        }
    }
    out
}

fn push_start(out: &mut String, start: &StartView) {
    let stage = match &start.stage {
        StartStage::WaitingToStart { text }
        | StartStage::Started { text }
        | StartStage::IdleAsReported { text }
        | StartStage::Unknown { text, .. } => clean(text),
        StartStage::StartFailed { last_error, text } => {
            format!("{}\n  last error: {}", clean(text), fact_text(last_error))
        }
    };
    out.push_str(&format!(
        "start of {} (read {}): {stage}\n",
        clean(&start.thread_id),
        utc(start.read_at)
    ));
    if let Some(reported) = &start.reported {
        out.push_str(&format!("{}\n", clean(&reported.label)));
        for setting in [&reported.provider, &reported.model, &reported.effort] {
            out.push_str(&format!("  {}\n", clean(&setting.text)));
        }
    }
    push_gaps(out, &start.gaps);
}

/// The text of a create's report. See `tests/new-thread/n18_cli.rs`.
pub fn render_report(report: &NewThreadReport) -> String {
    let mut out = format!(
        "openwiz new · op {} · {}\n",
        report.op_id,
        project_line(&report.project_ref, report.project.as_ref())
    );
    if let Some(source) = &report.source {
        out.push_str(&format!("source: {}\n", source_text(source)));
    }
    out.push_str(&requested_line(&report.requested));
    out.push_str(&format!("{}\n", clean(&report.summary)));
    out.push_str(&attach::lines(&report.attachments));
    match &report.result {
        NewThreadResult::Outcome {
            outcome,
            replayed,
            recovered,
        } => {
            if let Outcome::Created { thread_id, .. } = outcome {
                out.push_str(&format!("thread: {}\n", clean(thread_id)));
            }
            if *replayed {
                out.push_str(
                    "replayed: this op id was used before; this is its recorded outcome, and no \
                     call was made\n",
                );
            }
            if *recovered {
                out.push_str(
                    "recovered: the process creating this op id stopped before recording an \
                     outcome; nothing was sent again\n",
                );
            }
            if let Outcome::Uncertain { .. } = outcome {
                out.push_str(&format!(
                    "next: nothing is retried, by itself or by OpenWiz (there is no retry); \
                     check it with openwiz new --status {}, and resolve it in CloudRoom\n",
                    report.op_id
                ));
            }
        }
        NewThreadResult::InProgress { .. } | NewThreadResult::ConflictingReuse { .. } => {}
    }
    if let Some(start) = &report.start {
        push_start(&mut out, start);
    }
    if let Some(effects) = &report.effects {
        push_effects(&mut out, effects, "worked out before the call");
    }
    push_gaps(&mut out, &report.gaps);
    out
}

/// The text of a status read: its op id, and the record's report, start and
/// observations.
pub fn render_status(status: &NewThreadStatus) -> String {
    let mut out = format!("openwiz new --status · op {}\n", status.op_id);
    let StatusResult::Recorded(recorded) = &status.result else {
        out.push_str(
            "no record: no create with this op id is in the new-thread record; nothing was \
             created with it from here\n",
        );
        return out;
    };
    out.push_str(&render_report(&recorded.report));
    if let Some(start) = &recorded.start {
        push_start(&mut out, start);
    }
    if let Some(observations) = &recorded.observations {
        out.push_str(&format!(
            "{} (read {}):\n",
            clean(&observations.text),
            utc(observations.read_at)
        ));
        if observations.threads.is_empty() {
            out.push_str("  no thread seen in the project since the attempt\n");
        }
        for thread in &observations.threads {
            let created = match &thread.created_at {
                Fact::Known(at) => utc(*at),
                Fact::Unknown(why) => format!("unknown ({})", clean(why)),
            };
            out.push_str(&format!(
                "  seen: {} · created {created}\n",
                clean(&thread.id)
            ));
        }
        push_gaps(&mut out, &observations.gaps);
    }
    out
}

/// The text of the options: each harness with its models and efforts, the
/// unavailable ones with their reason, and no default.
pub fn render_options(options: &CloudOptions) -> String {
    let mut out = format!(
        "openwiz models · read {} · {}\n",
        utc(options.read_at),
        clean(&options.label)
    );
    if let OptionsState::NoOptions { text, .. } = &options.state {
        out.push_str(&format!("no options: {}\n", clean(text)));
    }
    for harness in &options.harnesses {
        out.push_str(&format!(
            "{} (spawn provider {})\n",
            clean(&harness.harness),
            clean(&harness.provider)
        ));
        for model in &harness.models {
            out.push_str(&format!(
                "  {}: {}\n",
                clean(&model.model),
                clean(&model.efforts.join(", "))
            ));
        }
    }
    for harness in &options.unavailable {
        out.push_str(&format!(
            "{}: {}\n",
            clean(&harness.harness),
            clean(&harness.text)
        ));
    }
    out
}
