use std::ffi::OsString;
use std::io::Write;
use std::ops::Deref;

use openwiz_core::attachment::{AttachmentId, FailReason, Kind, Readiness};
use openwiz_core::messaging::record::StoreError;
use openwiz_core::model::Timestamp;
use openwiz_core::ops::Clock;
use openwiz_store::attachments::report::{self, RemoveResult, StagedView};
use openwiz_store::attachments::{AttachmentStore, Claims, FreeSpace};
use serde::Serialize;

use crate::{EXIT_OK, EXIT_USAGE, UsageError, age, clean, help, utc};

/// `rm`: an id was not found. The other ids were removed.
pub const EXIT_ATTACHMENT_NOT_FOUND: i32 = 3;
/// `add`: a path failed. The others are reported, and are ready.
pub const EXIT_ATTACHMENT_FAILED: i32 = 5;
/// The attachment store could not be opened, read or written, or the free
/// space on its volume measured.
pub const EXIT_ATTACHMENT_STORE_UNAVAILABLE: i32 = 10;
/// `rm`: refused, because its operation is dispatching or uncertain. Its
/// bytes are kept.
pub const EXIT_ATTACHMENT_REFUSED: i32 = 14;

/// The schema ids of the three reports: the store's.
pub use openwiz_store::attachments::report::{ADD_SCHEMA_ID, LIST_SCHEMA_ID, REMOVE_SCHEMA_ID};

/// What `openwiz attachment` needs from the program around it.
pub struct AttachmentContext<'a> {
    /// Opens (and creates) the store. Called once the command line is valid.
    pub open_store: &'a dyn Fn() -> Result<AttachmentStore, StoreError>,
    /// Opens the store only if it exists (`ls`): `None` creates nothing.
    pub open_store_read_only: &'a dyn Fn() -> Result<Option<AttachmentStore>, StoreError>,
    pub clock: &'a dyn Clock,
    pub free_space: &'a dyn FreeSpace,
    pub claims: &'a dyn Claims,
}

/// A parsed `openwiz attachment` command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttachmentCommand {
    Add { paths: Vec<OsString>, json: bool },
    Ls { json: bool },
    Rm { ids: Vec<AttachmentId>, json: bool },
    Help,
}

/// Parses the arguments after `attachment`. Every option comes before
/// `--`; everything after it is a path or an id.
pub fn parse_attachment(args: &[OsString]) -> Result<AttachmentCommand, UsageError> {
    let Some((command, rest)) = args.split_first() else {
        return Err(UsageError(
            "attachment needs a command: add, ls or rm".to_owned(),
        ));
    };
    let mut json = false;
    let mut positionals: Vec<OsString> = Vec::new();
    let mut rest = rest.iter();
    while let Some(arg) = rest.next() {
        if arg == "--" {
            positionals.extend(rest.by_ref().cloned());
            break;
        }
        match arg.to_str() {
            Some("--help" | "-h") => return Ok(AttachmentCommand::Help),
            Some("--json") => json = true,
            Some(flag) if flag.starts_with("--json=") => {
                return Err(UsageError("--json takes no value".to_owned()));
            }
            Some(flag) if flag.len() > 1 && flag.starts_with('-') => {
                return Err(UsageError(format!(
                    "unknown option {} (put -- before a path that starts with -)",
                    clean(flag)
                )));
            }
            None if arg.as_encoded_bytes().starts_with(b"-") => {
                return Err(UsageError(
                    "unknown option (put -- before a path that starts with -)".to_owned(),
                ));
            }
            _ => positionals.push(arg.clone()),
        }
    }
    match command.to_str() {
        Some("--help" | "-h") => Ok(AttachmentCommand::Help),
        Some("add") if positionals.is_empty() => Err(UsageError(
            "attachment add needs at least one path".to_owned(),
        )),
        Some("add") => Ok(AttachmentCommand::Add {
            paths: positionals,
            json,
        }),
        Some("ls") if !positionals.is_empty() => {
            Err(UsageError("attachment ls takes no path or id".to_owned()))
        }
        Some("ls") => Ok(AttachmentCommand::Ls { json }),
        Some("rm") if positionals.is_empty() => Err(UsageError(
            "attachment rm needs at least one attachment id".to_owned(),
        )),
        Some("rm") => {
            let ids = positionals
                .iter()
                .map(|id| id.to_str().and_then(|id| AttachmentId::parse(id).ok()))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| {
                    UsageError(
                        "attachment rm takes attachment ids, as attachment ls shows them"
                            .to_owned(),
                    )
                })?;
            Ok(AttachmentCommand::Rm { ids, json })
        }
        Some(other) => Err(UsageError(format!(
            "unknown attachment command {} (add, ls or rm)",
            clean(other)
        ))),
        None => Err(UsageError(
            "unknown attachment command (add, ls or rm)".to_owned(),
        )),
    }
}

/// Runs `openwiz attachment <args>` (the arguments after `attachment`) and
/// returns the exit code.
pub fn run_attachment(
    args: &[OsString],
    context: AttachmentContext<'_>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    let command = match parse_attachment(args) {
        Ok(command) => command,
        Err(UsageError(message)) => {
            let _ = writeln!(
                stderr,
                "openwiz: {}; nothing was staged or removed (see openwiz --help)",
                clean(&message)
            );
            return EXIT_USAGE;
        }
    };
    let now = context.clock.now();
    let result = match command {
        AttachmentCommand::Help => {
            let _ = stdout.write_all(help().as_bytes());
            return EXIT_OK;
        }
        AttachmentCommand::Add { paths, json } => add(&paths, now, &context).map(|report| {
            let code = report.exit_code();
            print(stdout, json, &report, report.text());
            code
        }),
        AttachmentCommand::Ls { json } => list(now, &context).map(|report| {
            print(stdout, json, &report, report.text());
            EXIT_OK
        }),
        AttachmentCommand::Rm { ids, json } => remove(&ids, now, &context).map(|report| {
            let code = report.exit_code();
            print(stdout, json, &report, report.text());
            code
        }),
    };
    result.unwrap_or_else(|StoreError(why)| {
        let _ = writeln!(
            stderr,
            "openwiz: the attachment store could not be used: {}; nothing was staged or removed",
            clean(&why)
        );
        EXIT_ATTACHMENT_STORE_UNAVAILABLE
    })
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

/// Opens the store and sweeps it: unfinished copies go, expired ones are
/// reported.
fn open(
    now: Timestamp,
    context: &AttachmentContext<'_>,
) -> Result<(AttachmentStore, Vec<StagedView>), StoreError> {
    let store = (context.open_store)()?;
    let expired = report::sweep(&store, now, context.claims)?;
    Ok((store, expired))
}

fn add(
    paths: &[OsString],
    now: Timestamp,
    context: &AttachmentContext<'_>,
) -> Result<AddReport, StoreError> {
    let (store, expired) = open(now, context)?;
    let mut report = stage_paths(&store, paths, now, context.free_space);
    report.0.expired = expired;
    Ok(report)
}

/// Stages each of `paths` into `store`, in order, and reports every one:
/// `attachment add`'s work, shared with `send` and `new`'s `--attach` (B2).
pub(crate) fn stage_paths(
    store: &AttachmentStore,
    paths: &[OsString],
    now: Timestamp,
    free_space: &dyn FreeSpace,
) -> AddReport {
    AddReport(report::stage_paths(store, paths, now, free_space))
}

fn list(now: Timestamp, context: &AttachmentContext<'_>) -> Result<ListReport, StoreError> {
    // Nothing was ever staged: nothing is made.
    if (context.open_store_read_only)()?.is_none() {
        return Ok(ListReport(report::ListReport::new()));
    }
    let (store, expired) = open(now, context)?;
    Ok(ListReport(report::list(
        &store,
        context.claims,
        now,
        expired,
    )?))
}

fn remove(
    ids: &[AttachmentId],
    now: Timestamp,
    context: &AttachmentContext<'_>,
) -> Result<RemoveReport, StoreError> {
    let (store, expired) = open(now, context)?;
    Ok(RemoveReport(report::remove(
        &store,
        ids,
        context.claims,
        expired,
    )))
}

/// Everything of an attachment but its id and its state: name, kind, size,
/// hash, the original name, when it became ready, its age if `with_age`, and
/// when it expires.
fn staged_text(view: &StagedView, with_age: bool) -> String {
    let a = &view.attachment;
    let mut out = format!(
        "{} · {} · {} bytes · sha256 {} · from {} · ready {}",
        a.name.as_str(),
        kind_label(a.kind),
        a.size_bytes,
        a.sha256.as_str(),
        clean(&a.display_name),
        utc(Timestamp(view.ready_at))
    );
    if with_age {
        out.push_str(&format!(" · age {}", age(view.age_ms)));
    }
    match view.expires_at {
        Some(at) => out.push_str(&format!(" · expires {}", utc(Timestamp(at)))),
        None => out.push_str(" · kept while its operation is in flight"),
    }
    out
}

fn kind_label(kind: Kind) -> &'static str {
    Kind::labels()
        .iter()
        .find(|(_, k)| *k == kind)
        .map_or("", |(label, _)| label)
}

fn reason_label(readiness: Readiness) -> &'static str {
    match readiness {
        Readiness::Failed { reason } => FailReason::labels()
            .iter()
            .find(|(_, r)| *r == reason)
            .map_or("", |(label, _)| label),
        _ => "",
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn expired_lines(out: &mut String, expired: &[StagedView]) {
    for view in expired {
        out.push_str(&format!(
            "expired {} · {}\n",
            view.attachment.id.as_str(),
            staged_text(view, false)
        ));
    }
}

/// `openwiz attachment add`'s report (`openwiz.attachment-add.v1`): the
/// store's , with the CLI's exit code and text.
#[derive(Clone, Debug, Serialize)]
#[serde(transparent)]
pub struct AddReport(pub report::AddReport);

impl Deref for AddReport {
    type Target = report::AddReport;

    fn deref(&self) -> &report::AddReport {
        &self.0
    }
}

impl AddReport {
    pub(crate) fn exit_code(&self) -> i32 {
        if !self.not_staged.is_empty() {
            EXIT_ATTACHMENT_STORE_UNAVAILABLE
        } else if !self.failed.is_empty() {
            EXIT_ATTACHMENT_FAILED
        } else {
            EXIT_OK
        }
    }

    pub(crate) fn text(&self) -> String {
        let total = self.ready.len() + self.failed.len() + self.not_staged.len();
        let mut lines: Vec<(usize, String)> = Vec::new();
        for r in &self.ready {
            lines.push((
                r.index,
                format!(
                    "{}: ready · {} · {}",
                    clean(&r.path),
                    r.staged.attachment.id.as_str(),
                    staged_text(&r.staged, false)
                ),
            ));
        }
        for f in &self.failed {
            lines.push((
                f.index,
                format!(
                    "{}: failed ({}) · {}",
                    clean(&f.path),
                    reason_label(f.readiness),
                    f.message
                ),
            ));
        }
        for n in &self.not_staged {
            lines.push((
                n.index,
                format!("{}: not staged · {}", clean(&n.path), clean(&n.error)),
            ));
        }
        lines.sort_by_key(|(index, _)| *index);
        let mut out = format!(
            "openwiz attachment add · {}: {} ready, {} failed, {} not staged\n",
            plural(total, "path", "paths"),
            self.ready.len(),
            self.failed.len(),
            self.not_staged.len()
        );
        for (_, line) in lines {
            out.push_str(&line);
            out.push('\n');
        }
        expired_lines(&mut out, &self.expired);
        out
    }
}

/// `openwiz attachment ls`'s report (`openwiz.attachment-list.v1`): the
/// store's , with the CLI's text.
#[derive(Clone, Debug, Serialize)]
#[serde(transparent)]
pub struct ListReport(pub report::ListReport);

impl Deref for ListReport {
    type Target = report::ListReport;

    fn deref(&self) -> &report::ListReport {
        &self.0
    }
}

impl ListReport {
    fn text(&self) -> String {
        let mut out = format!(
            "openwiz attachment ls · {} ready, {} broken, {} expired\n",
            self.attachments.len(),
            self.broken.len(),
            self.expired.len()
        );
        for view in &self.attachments {
            out.push_str(&format!(
                "{} · ready · {}\n",
                view.attachment.id.as_str(),
                staged_text(view, true)
            ));
        }
        for b in &self.broken {
            out.push_str(&format!(
                "{} · failed ({}) · {}\n",
                b.id.as_str(),
                reason_label(b.readiness),
                b.message
            ));
        }
        expired_lines(&mut out, &self.expired);
        if self.attachments.is_empty() && self.broken.is_empty() {
            out.push_str("no attachments are staged\n");
        }
        out
    }
}

/// What `rm` did with one id, in words.
fn result_words(result: RemoveResult) -> &'static str {
    match result {
        RemoveResult::Removed => "removed",
        RemoveResult::Expired => "expired",
        RemoveResult::Refused => "refused",
        RemoveResult::NotFound => "not found",
        RemoveResult::NotRemoved => "not removed",
    }
}

/// `openwiz attachment rm`'s report (`openwiz.attachment-remove.v1`): the
/// store's , with the CLI's exit code and text.
#[derive(Clone, Debug, Serialize)]
#[serde(transparent)]
pub struct RemoveReport(pub report::RemoveReport);

impl Deref for RemoveReport {
    type Target = report::RemoveReport;

    fn deref(&self) -> &report::RemoveReport {
        &self.0
    }
}

impl RemoveReport {
    fn count(&self, result: RemoveResult) -> usize {
        self.ids.iter().filter(|e| e.result == result).count()
    }

    fn exit_code(&self) -> i32 {
        if self.count(RemoveResult::NotRemoved) > 0 {
            EXIT_ATTACHMENT_STORE_UNAVAILABLE
        } else if self.count(RemoveResult::Refused) > 0 {
            EXIT_ATTACHMENT_REFUSED
        } else if self.count(RemoveResult::NotFound) > 0 {
            EXIT_ATTACHMENT_NOT_FOUND
        } else {
            EXIT_OK
        }
    }

    fn text(&self) -> String {
        let mut out = format!(
            "openwiz attachment rm · {}: {} removed, {} expired, {} refused, {} not found, {} \
             not removed\n",
            plural(self.ids.len(), "id", "ids"),
            self.count(RemoveResult::Removed),
            self.count(RemoveResult::Expired),
            self.count(RemoveResult::Refused),
            self.count(RemoveResult::NotFound),
            self.count(RemoveResult::NotRemoved)
        );
        for entry in &self.ids {
            out.push_str(&format!(
                "{}: {} · {}\n",
                entry.id.as_str(),
                result_words(entry.result),
                clean(&entry.message)
            ));
        }
        expired_lines(&mut out, &self.expired);
        out
    }
}
