//! `openwiz send`: parses the command line, calls the core's `send`, and
//! prints its report as text or as `openwiz.messaging.v1` JSON.
//!
//! Text and `--json` print the same report. Usage errors and an unusable
//! send record go to stderr as one `openwiz: ` line, with stdout empty. The
//! exit codes are the `EXIT_*` constants here and in the crate root,
//! documented in [`help`](crate::help).

use std::fs::File;
use std::io::{Read, Write};

use openwiz_core::messaging::lookup::{LookupReport, LookupResult, lookup};
use openwiz_core::messaging::preview::{PreviewReport, PreviewStatus, preview};
use openwiz_core::messaging::record::{SendStore, StoreError};
use openwiz_core::messaging::send::{
    Delivery, FailReason, Messenger, STEER_WITH_ATTACHMENTS, SendError, SendReport, SendRequest,
    SendResult, send_with,
};
use openwiz_core::messaging::{InvalidRequest, MAX_TEXT_BYTES, Mode, OpId, validate_text};
use openwiz_core::model::{Fact, SessionId};
use openwiz_core::ops::Clock;
use openwiz_core::prompt::{NoAttachments, StagedAttachments};
use openwiz_store::AttachmentStore;
use openwiz_store::attachments::Df;

use crate::attach::{self, AttachArg, Prepared};
use crate::{EXIT_NOT_FOUND, EXIT_OK, EXIT_USAGE, UsageError, age, clean, help, source_text};

/// `send`: the message was queued, or sent (the provider accepted it; never
/// that the work completed).
pub const EXIT_DELIVERED: i32 = 0;

pub const EXIT_SEND_FAILED: i32 = 5;
/// `send`: uncertain. The message may or may not have been accepted; the
/// text is kept in the send record.
pub const EXIT_UNCERTAIN: i32 = 6;
/// `send`: another process is sending with this op id. No call was made.
pub const EXIT_IN_PROGRESS: i32 = 7;
/// `send`: this op id is bound to a different target, mode or text. No
/// call was made.
pub const EXIT_CONFLICTING_REUSE: i32 = 8;
/// `send`: an earlier send of this text to this target is uncertain, or the
/// same send (target, mode and text) is still in progress under another op
/// id, and `--accept-possible-duplicate` was not given. Nothing was sent.
pub const EXIT_POSSIBLE_DUPLICATE: i32 = 9;
/// `send`: the send record could not be opened, read or written. Nothing
/// was sent.
pub const EXIT_RECORD_UNAVAILABLE: i32 = 10;

/// `send --check`: the send record has no send with this op id to this
/// target. Nothing was sent to it with this op id from this record.
pub const EXIT_NO_RECORD: i32 = 11;

/// Reads the target and shows what a send would do; no text, no claim, no
/// call.
pub const PREVIEW: &str = "--preview";
/// Shows the send record of `--op-id` for the target; no claim, no call.
pub const CHECK: &str = "--check";

/// The acknowledgement that a send may resume a queue that appears paused.
pub const RESUME_PAUSED_QUEUE: &str = "--resume-paused-queue";
/// The acknowledgement that a send may duplicate an earlier uncertain one,
/// or the same send still in progress under another op id.
pub const ACCEPT_POSSIBLE_DUPLICATE: &str = "--accept-possible-duplicate";

/// What `openwiz send` needs from the program around it.
pub struct SendContext<'a> {
    pub messenger: &'a dyn Messenger,
    /// Opens the send record. Called only once the command line is valid.
    pub open_store: &'a dyn Fn() -> Result<Box<dyn SendStore>, StoreError>,
    /// Opens the send record for reading only (`--check`): creates nothing,
    /// and `None` when it does not exist yet.
    pub open_store_read_only: &'a dyn Fn() -> Result<Option<Box<dyn SendStore>>, StoreError>,
    pub clock: &'a dyn Clock,
    /// Where the text comes from when it is not on the command line.
    pub stdin: &'a mut dyn Read,
    /// A terminal is never read for the text: that would wait for typing.
    pub stdin_is_terminal: bool,
    /// Makes a new op id when `--op-id` is not given.
    pub new_op_id: &'a dyn Fn() -> Result<OpId, String>,
}

/// Opens the attachment store (B2): called only for a send or a create
/// with `--attach` or `--attachment`, once the command line is valid.
pub type OpenAttachments<'a> = &'a dyn Fn() -> Result<AttachmentStore, StoreError>;

/// The opener of a caller with no attachment store: any `--attach` or
/// `--attachment` is refused before anything is read or sent.
pub fn no_attachment_store() -> Result<AttachmentStore, StoreError> {
    Err(StoreError(
        "no attachment store is available to this command".to_owned(),
    ))
}

/// A parsed `openwiz send` command line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SendArgs {
    pub id: String,
    /// `None`: read the text from stdin.
    pub text: Option<String>,
    pub steer: bool,
    pub op_id: Option<String>,
    pub json: bool,
    pub resume_paused_queue: bool,
    pub accept_possible_duplicate: bool,
    /// `--preview`: read the target and show what a send would do.
    pub preview: bool,
    /// `--check`: show the send record of `--op-id`.
    pub check: bool,
    /// `--attach <path>` and `--attachment <id>`, in command-line order (B2).
    pub attachments: Vec<AttachArg>,
}

/// What `openwiz send` was asked to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SendCommand {
    Send(SendArgs),
    Help,
}

/// Parses the arguments after `send`. Every option comes before `--`;
/// everything after it is the id and the text.
pub fn parse_send(args: &[String]) -> Result<SendCommand, UsageError> {
    let mut parsed = SendArgs::default();
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
        if attach::is_flag(flag) {
            let value = match inline {
                Some(value) => value.to_owned(),
                None => args
                    .next()
                    .cloned()
                    .ok_or_else(|| UsageError(format!("{flag} needs a value")))?,
            };
            let parsed_arg = attach::parse_flag(flag, value).expect("an attachment flag")?;
            parsed.attachments.push(parsed_arg);
            continue;
        }
        match flag {
            "--help" | "-h" => return Ok(SendCommand::Help),
            "--op-id" => {
                let value = match inline {
                    Some(value) => value.to_owned(),
                    None => args
                        .next()
                        .cloned()
                        .ok_or_else(|| UsageError("--op-id needs a value".to_owned()))?,
                };
                parsed.op_id = Some(value);
            }
            "--steer"
            | "--json"
            | RESUME_PAUSED_QUEUE
            | ACCEPT_POSSIBLE_DUPLICATE
            | PREVIEW
            | CHECK
                if inline.is_some() =>
            {
                return Err(UsageError(format!("{flag} takes no value")));
            }
            "--steer" => parsed.steer = true,
            "--json" => parsed.json = true,
            RESUME_PAUSED_QUEUE => parsed.resume_paused_queue = true,
            ACCEPT_POSSIBLE_DUPLICATE => parsed.accept_possible_duplicate = true,
            PREVIEW => parsed.preview = true,
            CHECK => parsed.check = true,
            other => {
                return Err(UsageError(format!(
                    "unknown option {} (put -- before text that starts with -)",
                    clean(other)
                )));
            }
        }
    }
    let mut positionals = positionals.into_iter();
    parsed.id = positionals
        .next()
        .ok_or_else(|| UsageError("send needs a session id".to_owned()))?;
    parsed.text = positionals.next();
    if positionals.next().is_some() {
        return Err(UsageError(
            "send takes one session id and one text (quote the text)".to_owned(),
        ));
    }
    check_read_only(&parsed)?;
    Ok(SendCommand::Send(parsed))
}

/// `--preview` and `--check` never send: they take no text and no option
/// that only a send uses.
fn check_read_only(args: &SendArgs) -> Result<(), UsageError> {
    let (form, refused) = match (args.preview, args.check) {
        (false, false) => return Ok(()),
        (true, true) => return Err(UsageError(format!("give {PREVIEW} or {CHECK}, not both"))),
        (true, false) => (
            PREVIEW,
            [
                (args.op_id.is_some(), "--op-id"),
                (args.resume_paused_queue, RESUME_PAUSED_QUEUE),
                (args.accept_possible_duplicate, ACCEPT_POSSIBLE_DUPLICATE),
            ],
        ),
        (false, true) => (
            CHECK,
            [
                (args.steer, "--steer"),
                (args.resume_paused_queue, RESUME_PAUSED_QUEUE),
                (args.accept_possible_duplicate, ACCEPT_POSSIBLE_DUPLICATE),
            ],
        ),
    };
    if args.text.is_some() {
        return Err(UsageError(format!("{form} takes no text: it never sends")));
    }
    if !args.attachments.is_empty() {
        return Err(UsageError(format!(
            "{form} takes no attachment: it never sends"
        )));
    }
    if let Some((_, flag)) = refused.iter().find(|(given, _)| *given) {
        return Err(UsageError(format!(
            "{form} takes no {flag}: it never sends"
        )));
    }
    if args.check && args.op_id.is_none() {
        return Err(UsageError(format!("{CHECK} needs --op-id <op id>")));
    }
    Ok(())
}

fn usage_error(stderr: &mut dyn Write, message: &str) -> i32 {
    let _ = writeln!(
        stderr,
        "openwiz: {}; nothing was sent (see openwiz --help)",
        clean(message)
    );
    EXIT_USAGE
}

/// Prints `value` as pretty JSON, or `text`.
fn print(stdout: &mut dyn Write, json: bool, value: &impl serde::Serialize, text: String) {
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

/// `openwiz send <id> --preview`: reads the target only. The send record is
/// never opened. Exits as a send would before its call.
fn run_preview(
    args: &SendArgs,
    context: &SendContext<'_>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    let target = SessionId(args.id.clone());
    let mode = if args.steer { Mode::Steer } else { Mode::Queue };
    match preview(context.messenger, context.clock, &target, mode) {
        Ok(report) => {
            print(stdout, args.json, &report, render_preview(&report));
            match (&report.status, &report.refusal) {
                (PreviewStatus::Ready, _) => EXIT_DELIVERED,
                (_, Some(refusal)) if refusal.reason == FailReason::NotFound => EXIT_NOT_FOUND,
                _ => EXIT_SEND_FAILED,
            }
        }
        Err(InvalidRequest(why)) => usage_error(stderr, &why),
    }
}

/// `openwiz send <id> --op-id <op id> --check`: reads the send record only,
/// through the read-only opener: nothing is created or written. No
/// `room-cli` call is made.
fn run_check(
    args: &SendArgs,
    context: &SendContext<'_>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    let target = SessionId(args.id.clone());
    if let Err(InvalidRequest(why)) = context.messenger.check_target_id(&target) {
        return usage_error(stderr, &why);
    }
    let op_id = match OpId::parse(args.op_id.as_deref().unwrap_or_default()) {
        Ok(op_id) => op_id,
        Err(InvalidRequest(why)) => return usage_error(stderr, &why),
    };
    let unusable = |stderr: &mut dyn Write, why: &str| {
        let _ = writeln!(
            stderr,
            "openwiz: the send record could not be used: {}; nothing was sent",
            clean(why)
        );
        EXIT_RECORD_UNAVAILABLE
    };
    // Read only: nothing is created, and no send record yet is "no record".
    let store = match (context.open_store_read_only)() {
        Ok(store) => store,
        Err(StoreError(why)) => return unusable(stderr, &why),
    };
    match lookup(store.as_deref(), context.clock, &target, &op_id) {
        Ok(report) => {
            print(stdout, args.json, &report, render_lookup(&report));
            match &report.result {
                LookupResult::NoRecord => EXIT_NO_RECORD,
                LookupResult::OtherTarget => EXIT_CONFLICTING_REUSE,
                LookupResult::Recorded { report } => exit_code(report),
            }
        }
        Err(StoreError(why)) => unusable(stderr, &why),
    }
}

/// Runs `openwiz send <args>` (the arguments after `send`) with no
/// attachment store, and returns the exit code: any `--attach` or
/// `--attachment` is refused ([`no_attachment_store`]).
pub fn run_send(
    args: &[String],
    context: SendContext<'_>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    run_send_with(args, context, &no_attachment_store, stdout, stderr)
}

/// Runs `openwiz send <args>` (the arguments after `send`), with the
/// attachment store `open_attachments` opens, and returns the exit code.
pub fn run_send_with(
    args: &[String],
    context: SendContext<'_>,
    open_attachments: OpenAttachments<'_>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    let usage = usage_error;
    let args = match parse_send(args) {
        Ok(SendCommand::Help) => {
            let _ = stdout.write_all(help().as_bytes());
            return EXIT_OK;
        }
        Ok(SendCommand::Send(args)) => args,
        Err(UsageError(message)) => return usage(stderr, &message),
    };
    if args.preview {
        return run_preview(&args, &context, stdout, stderr);
    }
    if args.check {
        return run_check(&args, &context, stdout, stderr);
    }
    let text = match args.text {
        Some(text) => text,
        None if context.stdin_is_terminal => {
            return usage(
                stderr,
                "give the text after the id, or pipe it on stdin (a terminal is never read)",
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
    if let Err(InvalidRequest(why)) = validate_text(&text) {
        return usage(stderr, &why);
    }
    let target = SessionId(args.id);
    if let Err(InvalidRequest(why)) = context.messenger.check_target_id(&target) {
        return usage(stderr, &why);
    }
    let given = match args.op_id.as_deref().map(OpId::parse).transpose() {
        Ok(given) => given,
        Err(InvalidRequest(why)) => return usage(stderr, &why),
    };
    if args.steer && !args.attachments.is_empty() {
        // Before anything is staged: Steer is never turned into Queue.
        return usage(stderr, STEER_WITH_ATTACHMENTS);
    }

    // B2: the attachments, staged or named, before the send record is
    // opened. A path that fails stops everything: nothing is sent.
    let mut ids = Vec::new();
    let mut attachment_store = None;
    if !args.attachments.is_empty() {
        let opened = match open_attachments() {
            Ok(opened) => opened,
            Err(e) => {
                let _ = writeln!(
                    stderr,
                    "openwiz: {}; nothing was sent",
                    attach::store_unusable(&e)
                );
                return EXIT_RECORD_UNAVAILABLE;
            }
        };
        match attach::prepare(&args.attachments, &opened, context.clock.now(), &Df) {
            Prepared::Ids(prepared) => ids = prepared,
            Prepared::Failed(report) => {
                let text = attach::failed_text(&report, "nothing was sent");
                print(stdout, args.json, &report, text);
                return match attach::failed_code(&report) {
                    EXIT_RECORD_UNAVAILABLE => EXIT_RECORD_UNAVAILABLE,
                    _ => EXIT_SEND_FAILED,
                };
            }
        }
        attachment_store = Some(opened);
    }
    let attachments: &dyn StagedAttachments = match &attachment_store {
        Some(store) => store,
        None => &NoAttachments,
    };

    let store = match (context.open_store)() {
        Ok(store) => store,
        Err(StoreError(why)) => {
            let _ = writeln!(
                stderr,
                "openwiz: the send record could not be opened: {}; nothing was sent",
                clean(&why)
            );
            return EXIT_RECORD_UNAVAILABLE;
        }
    };
    let op_id = match given {
        Some(op_id) => op_id,
        None => match (context.new_op_id)() {
            Ok(op_id) => {
                // Shown before anything is sent, so a send that is cut short
                // can still be checked with --op-id.
                let _ = writeln!(
                    stderr,
                    "openwiz: op id {op_id} (use --op-id {op_id} to check this send again)"
                );
                op_id
            }
            Err(why) => {
                let _ = writeln!(
                    stderr,
                    "openwiz: no op id could be made: {}; nothing was sent",
                    clean(&why)
                );
                return EXIT_RECORD_UNAVAILABLE;
            }
        },
    };
    let mut request = SendRequest::new(target, text, op_id);
    request.mode = if args.steer { Mode::Steer } else { Mode::Queue };
    request.resume_paused_queue = args.resume_paused_queue;
    request.accept_possible_duplicate = args.accept_possible_duplicate;
    request.prompt.attachments = ids;

    match send_with(
        context.messenger,
        store.as_ref(),
        attachments,
        context.clock,
        &request,
    ) {
        Ok(report) => {
            print(stdout, args.json, &report, render_send(&report));
            exit_code(&report)
        }
        Err(SendError::Invalid(InvalidRequest(why))) => usage(stderr, &why),
        Err(SendError::Store(StoreError(why))) => {
            let _ = writeln!(
                stderr,
                "openwiz: the send record could not be used: {}",
                clean(&why)
            );
            EXIT_RECORD_UNAVAILABLE
        }
    }
}

/// The exit code of a send report.
pub fn exit_code(report: &SendReport) -> i32 {
    match &report.result {
        SendResult::Outcome { delivery, .. } => match delivery {
            Delivery::Queued { .. } | Delivery::Accepted { .. } => EXIT_DELIVERED,
            Delivery::Failed {
                reason: FailReason::NotFound,
                ..
            } => EXIT_NOT_FOUND,
            Delivery::Failed { .. } => EXIT_SEND_FAILED,
            Delivery::Uncertain { .. } => EXIT_UNCERTAIN,
        },
        SendResult::InProgress { .. } => EXIT_IN_PROGRESS,
        SendResult::ConflictingReuse { .. } => EXIT_CONFLICTING_REUSE,
        SendResult::PossibleDuplicateRefused { .. } => EXIT_POSSIBLE_DUPLICATE,
    }
}

/// The text of a send report. See the text contract in
/// `tests/messaging/m14_cli.rs`.
pub fn render_send(report: &SendReport) -> String {
    let mut out = format!(
        "openwiz send · op {} · {} · {}\n",
        report.op_id,
        clean(&report.target.id.0),
        fact_text(&report.target.title)
    );
    if let Some(source) = &report.source {
        out.push_str(&format!("source: {}\n", source_text(source)));
    }
    if let Some(effects) = &report.effects {
        let effect = match &effects.effect {
            Fact::Known(effect) => effect.text.clone(),
            Fact::Unknown(why) => format!("unknown ({})", clean(why)),
        };
        let paused = match &effects.paused_queue {
            Fact::Known(paused) => paused.text.clone(),
            Fact::Unknown(why) => format!("unknown ({})", clean(why)),
        };
        out.push_str(&format!(
            "effects (read {} before sending): {effect}\npaused queue: {paused}\n",
            age(effects.read_age_ms)
        ));
    }
    out.push_str(&format!("{}\n", clean(&report.summary)));
    out.push_str(&attach::lines(&report.attachments));
    match &report.result {
        SendResult::Outcome {
            delivery,
            replayed,
            recovered,
        } => {
            if let Delivery::Queued {
                queued_message_id, ..
            } = delivery
            {
                out.push_str(&format!("queued message: {}\n", clean(queued_message_id)));
            }
            if *replayed {
                out.push_str(
                    "replayed: this op id was used before; this is its recorded outcome, and \
                     no call was made\n",
                );
            }
            if *recovered {
                out.push_str(
                    "recovered: the sender of this op id stopped before recording an outcome; \
                     nothing was resent\n",
                );
            }
            if let Delivery::Failed {
                reason: FailReason::PausedQueueNotAcknowledged,
                ..
            } = delivery
            {
                out.push_str(&format!(
                    "next: to resume the paused queue and send, run again with \
                     {RESUME_PAUSED_QUEUE} (a new op id)\n"
                ));
            }
            if let Delivery::Uncertain { .. } = delivery {
                out.push_str(&format!(
                    "next: nothing is resent by itself; a new send of this text to this target \
                     needs {ACCEPT_POSSIBLE_DUPLICATE}\n"
                ));
            }
        }
        SendResult::PossibleDuplicateRefused { earlier } => {
            for send in earlier {
                if send.in_flight {
                    out.push_str(&format!(
                        "earlier send still in progress: op {} ({}), no outcome yet\n",
                        send.op_id,
                        send.mode.requested()
                    ));
                } else {
                    out.push_str(&format!(
                        "earlier uncertain send: op {} ({}), no reconciliation read\n",
                        send.op_id,
                        send.mode.requested()
                    ));
                }
            }
            out.push_str(&format!(
                "next: to send anyway, knowing it may duplicate the earlier send, run again \
                 with {ACCEPT_POSSIBLE_DUPLICATE}\n"
            ));
        }
        SendResult::InProgress { .. } | SendResult::ConflictingReuse { .. } => {}
    }
    for gap in &report.gaps {
        out.push_str(&format!(
            "gap: {} · {}\n",
            clean(&gap.subject),
            clean(&gap.detail)
        ));
    }
    out
}

/// A new op id: `op-` and 32 lowercase hex digits from the operating
/// system's random source.
pub fn random_op_id() -> Result<OpId, String> {
    let mut bytes = [0u8; 16];
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| format!("the random source could not be read: {e}"))?;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    OpId::parse(&format!("op-{hex}")).map_err(|e| e.0)
}

/// The text of a preview: like a send's, with `--preview` on line 1 and no
/// outcome.
pub fn render_preview(report: &PreviewReport) -> String {
    let mut out = format!(
        "openwiz send --preview · {} · {}\n",
        clean(&report.target.id.0),
        fact_text(&report.target.title)
    );
    if let Some(effects) = &report.effects {
        let effect = match &effects.effect {
            Fact::Known(effect) => effect.text.clone(),
            Fact::Unknown(why) => format!("unknown ({})", clean(why)),
        };
        let paused = match &effects.paused_queue {
            Fact::Known(paused) => paused.text.clone(),
            Fact::Unknown(why) => format!("unknown ({})", clean(why)),
        };
        out.push_str(&format!(
            "effects (read {} ago): {effect}\npaused queue: {paused}\n",
            age(effects.read_age_ms)
        ));
    }
    out.push_str(&format!("{}\n", clean(&report.summary)));
    match (&report.status, &report.refusal) {
        (PreviewStatus::NeedsAcknowledgement, Some(refusal)) => out.push_str(&format!(
            "without {RESUME_PAUSED_QUEUE}: {}\nnext: to resume the paused queue and send, send \
             with {RESUME_PAUSED_QUEUE}\n",
            clean(&refusal.text)
        )),
        (_, Some(refusal)) => {
            out.push_str(&format!("refused: {}\n", clean(&refusal.text)));
        }
        (_, None) => {}
    }
    for gap in &report.gaps {
        out.push_str(&format!(
            "gap: {} · {}\n",
            clean(&gap.subject),
            clean(&gap.detail)
        ));
    }
    out
}

/// The text of a lookup: one line naming it, its summary, and the record's
/// report as `openwiz send` prints it.
pub fn render_lookup(report: &LookupReport) -> String {
    let mut out = format!(
        "openwiz send --check · op {} · {}\n{}\n",
        report.op_id,
        clean(&report.target.0),
        clean(&report.summary)
    );
    if let LookupResult::Recorded { report } = &report.result {
        out.push_str(&render_send(report));
    }
    out
}

fn fact_text(value: &Fact<String>) -> String {
    match value {
        Fact::Known(text) => clean(text),
        Fact::Unknown(why) => format!("unknown ({})", clean(why)),
    }
}
