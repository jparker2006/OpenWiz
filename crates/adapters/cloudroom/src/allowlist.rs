use openwiz_core::attachment::{Kind, StagedAttachment};
use openwiz_core::messaging::{Mode, validate_text};
use openwiz_core::prompt::attachment_allowance;

use crate::new_thread::{CLI_REASONING_LEVELS, HARNESS_PROVIDERS};

/// A session id that is safe to pass as one argument: it cannot be read as
/// a flag or split into several arguments.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ThreadId(String);

/// The longest id accepted.
pub const THREAD_ID_MAX_LEN: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidThreadId(pub String);

impl ThreadId {
    /// Accepts 1 to [`THREAD_ID_MAX_LEN`] ASCII letters, digits, `_` and `-`,
    /// not starting with `-`.
    pub fn parse(id: &str) -> Result<Self, InvalidThreadId> {
        let valid = !id.is_empty()
            && id.len() <= THREAD_ID_MAX_LEN
            && !id.starts_with('-')
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        if valid {
            Ok(Self(id.to_owned()))
        } else {
            Err(InvalidThreadId(id.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why a `thread spawn` could not be built: in OpenWiz's words, never the
/// value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidSpawn(pub String);

/// The arguments of one `thread spawn`, each checked when it is built, so
/// an [`Invocation::ThreadSpawn`] can only carry safe ones:
/// - the project id: a [`ThreadId`]-shaped id;
/// - the provider: one of the verified `spawn --provider` ids
///   ([`HARNESS_PROVIDERS`]);
/// - the model: 1 to 128 printable ASCII characters, no space, not
///   starting with `-`;
/// - the effort: one of the levels `thread spawn --help` lists
///   ([`CLI_REASONING_LEVELS`]);
/// - the request id: `ow1-` and 40 lowercase hex digits, as OpenWiz derives
///   it;
/// - the prompt: `send`'s text rules (not empty or only whitespace, no NUL,
///   at most 16 KiB);
/// - the attachments (B2): staged attachments only, in the order passed
///   (the core's dispatch order), through [`attachment_args`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnArgs {
    project: ThreadId,
    provider: String,
    model: String,
    effort: String,
    request_id: String,
    prompt: String,
    attachments: Vec<StagedAttachment>,
}

impl SpawnArgs {
    pub fn new(
        project_id: &str,
        provider: &str,
        model: &str,
        effort: &str,
        request_id: &str,
        prompt: &str,
    ) -> Result<Self, InvalidSpawn> {
        let refuse = |why: &str| Err(InvalidSpawn(why.to_owned()));
        let Ok(project) = ThreadId::parse(project_id) else {
            return refuse("the project id is not safe to pass as one argument");
        };
        if !HARNESS_PROVIDERS.iter().any(|(_, p)| *p == provider) {
            return refuse("the provider is not a verified spawn provider");
        }
        let model_ok = !model.is_empty()
            && model.len() <= MODEL_MAX_LEN
            && !model.starts_with('-')
            && model.bytes().all(|b| b.is_ascii_graphic());
        if !model_ok {
            return refuse("the model is not safe to pass as one argument");
        }
        if !CLI_REASONING_LEVELS.contains(&effort) {
            return refuse("the effort is not a level thread spawn lists");
        }
        let request_id_ok = request_id.strip_prefix("ow1-").is_some_and(|hex| {
            hex.len() == 40 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        });
        if !request_id_ok {
            return refuse("the request id is not ow1- and 40 lowercase hex digits");
        }
        if validate_text(prompt).is_err() {
            return refuse("the prompt does not follow the text rules");
        }
        Ok(Self {
            project,
            provider: provider.to_owned(),
            model: model.to_owned(),
            effort: effort.to_owned(),
            request_id: request_id.to_owned(),
            prompt: prompt.to_owned(),
            attachments: vec![],
        })
    }

    /// The same arguments, carrying `attachments`, in this order.
    pub fn with_attachments(self, attachments: Vec<StagedAttachment>) -> Self {
        Self {
            attachments,
            ..self
        }
    }
}

/// **The one place** that builds `--file` and `--image` (B2, AD11): for each
/// staged attachment, in the order given (the core's dispatch order: files,
/// then images), `--file <path>` or `--image <path>`, the flag by its kind
/// and the path its
/// [`StagedPath`](openwiz_core::attachment::StagedPath). `tell` and `spawn` append the same
/// tail; each keeps its own text placement (`tell`'s `--` and positional
/// text, `spawn`'s `--prompt`), after it.
pub fn attachment_args(staged: &[StagedAttachment]) -> Vec<String> {
    let mut tail = Vec::with_capacity(2 * staged.len());
    for a in staged {
        let flag = match a.attachment.kind {
            Kind::File => "--file",
            Kind::Image => "--image",
        };
        tail.push(flag.to_owned());
        tail.push(a.path.as_str().to_owned());
    }
    tail
}

/// The extra time a call carrying `staged` may take (B2's proposed
/// [`attachment_allowance`]).
fn allowance_of(staged: &[StagedAttachment]) -> std::time::Duration {
    let total = staged
        .iter()
        .map(|a| a.attachment.size_bytes)
        .fold(0, u64::saturating_add);
    attachment_allowance(total)
}

/// The longest model passed.
const MODEL_MAX_LEN: usize = 128;

/// One allowed `room-cli` invocation: a read, or one of the two writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Invocation {
    /// `room-cli --version`
    Version,
    /// `room-cli thread list --json`, with `--include-hidden` and
    /// `--include-archived` only on request.
    ThreadList {
        include_hidden: bool,
        include_archived: bool,
    },
    /// `room-cli project list --json --include-personal`, with `--hidden`
    /// only on request.
    ProjectList {
        hidden: bool,
    },
    /// `room-cli machine list --json`
    MachineList,
    /// `room-cli thread show <id> --json`
    ThreadShow(ThreadId),
    /// `room-cli thread output <id> --json`
    ThreadOutput(ThreadId),
    /// The one write: `room-cli thread tell <id> --mode <queue|steer> --json
    /// -- <text>`, with the text as one argument, byte for byte.
    ///
    /// **Documented, not observed; the argument shape must be rechecked on
    /// the owner's endpoint before any live send.** It follows the
    /// `room-cli` 0.43.1 help and guide as read during planning
    /// (2026-10-23): `thread tell <id> <message>`, `--mode
    /// steer|queue|auto`, `--json`. OpenWiz always passes `--mode` (never
    /// `auto`, never the location default) and none of `tell`'s other
    /// flags. The `--` before the text is OpenWiz's choice, so that text
    /// starting with `-` can never be read as a flag; whether 0.43.1 honours
    /// `--` as the end of options is undocumented, and is the first item of
    /// the pre-live recheck.
    ///
    /// **With attachments (B2, sheet the check):** `room-cli thread tell <id>
    /// --mode queue --json [--file|--image <staged path>]… -- <text>`, the
    /// tail from [`attachment_args`] before the `--`. Never with steer: the
    /// core refuses Steer with attachments before anything (the check).
    ThreadTell {
        id: ThreadId,
        mode: Mode,
        text: String,
        attachments: Vec<StagedAttachment>,
    },

    CloudStatus,

    ThreadSpawn(SpawnArgs),
}

impl Invocation {
    /// The staged attachments this call carries: none for a read.
    fn staged(&self) -> &[StagedAttachment] {
        match self {
            Invocation::ThreadTell { attachments, .. } => attachments,
            Invocation::ThreadSpawn(args) => &args.attachments,
            _ => &[],
        }
    }

    /// How much longer than a read this call may take: the attachments'
    /// allowance (B2), zero for everything else.
    pub fn allowance(&self) -> std::time::Duration {
        allowance_of(self.staged())
    }

    /// The exact arguments passed to `room-cli`, after the program name.
    pub fn argv(&self) -> Vec<String> {
        let tail = attachment_args(self.staged());
        let words: Vec<&str> = match self {
            Self::Version => vec!["--version"],
            Self::ThreadList {
                include_hidden,
                include_archived,
            } => {
                let mut w = vec!["thread", "list", "--json"];
                if *include_hidden {
                    w.push("--include-hidden");
                }
                if *include_archived {
                    w.push("--include-archived");
                }
                w
            }
            Self::ProjectList { hidden } => {
                let mut w = vec!["project", "list", "--json", "--include-personal"];
                if *hidden {
                    w.push("--hidden");
                }
                w
            }
            Self::MachineList => vec!["machine", "list", "--json"],
            Self::ThreadShow(id) => vec!["thread", "show", id.as_str(), "--json"],
            Self::ThreadOutput(id) => vec!["thread", "output", id.as_str(), "--json"],
            Self::ThreadTell { id, mode, text, .. } => {
                let mut w = vec![
                    "thread",
                    "tell",
                    id.as_str(),
                    "--mode",
                    mode.as_str(),
                    "--json",
                ];
                w.extend(tail.iter().map(String::as_str));
                w.extend(["--", text.as_str()]);
                w
            }
            Self::CloudStatus => vec!["cloud", "status", "--json"],
            Self::ThreadSpawn(a) => {
                let mut w = vec![
                    "thread",
                    "spawn",
                    "--project",
                    a.project.as_str(),
                    "--machine",
                    "cloud",
                    "--provider",
                    a.provider.as_str(),
                    "--model",
                    a.model.as_str(),
                    "--reasoning-level",
                    a.effort.as_str(),
                    "--request-id",
                    a.request_id.as_str(),
                    "--json",
                ];
                w.extend(tail.iter().map(String::as_str));
                w.extend(["--prompt", a.prompt.as_str()]);
                w
            }
        };
        words.into_iter().map(str::to_owned).collect()
    }
}
