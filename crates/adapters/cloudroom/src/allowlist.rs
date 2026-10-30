//! The only `room-cli` invocations OpenWiz may make. All of them are reads.
//!
//! Never `--work-status`, `--git-diff`, `thread log`, or any mutating
//! subcommand. The runner must start `room-cli` only from an
//! [`Invocation`] and its [`Invocation::argv`]. P8(a) checks the sources
//! for that as a tripwire; it is not a proof.

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

/// One allowed `room-cli` read.
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
    ProjectList { hidden: bool },
    /// `room-cli machine list --json`
    MachineList,
    /// `room-cli thread show <id> --json`
    ThreadShow(ThreadId),
    /// `room-cli thread output <id> --json`
    ThreadOutput(ThreadId),
}

impl Invocation {
    /// The exact arguments passed to `room-cli`, after the program name.
    pub fn argv(&self) -> Vec<String> {
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
        };
        words.into_iter().map(str::to_owned).collect()
    }
}
