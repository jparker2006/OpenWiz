//! Output and run limits.
//!
//! The first six values are fixed for 0.0.1. `MAX_LIMIT` and
//! `FIELD_MAX_CHARS` are provisional row and field caps.

use std::time::Duration;

/// `purpose.text` is cut to this many characters (Unicode scalar values).
pub const PURPOSE_MAX_CHARS: usize = 120;

/// `show` prints at most this many bytes of final output, cut on a character boundary.
pub const FINAL_OUTPUT_MAX_BYTES: usize = 4 * 1024;

/// At most this many detail reads (`thread show`, `thread output`) run at once.
pub const MAX_CONCURRENT_DETAIL_READS: usize = 4;

/// One run of `ls` or `show` finishes within this budget, complete or not.
pub const RUN_BUDGET: Duration = Duration::from_secs(30);

/// Each single read gives up after this long, inside the run budget, and is
/// never retried. The adapter applies it to every read.
pub const READ_TIMEOUT: Duration = Duration::from_secs(15);

/// Rows per page when `--limit` is not given.
pub const DEFAULT_LIMIT: usize = 100;

/// The largest page `--limit` may ask for. Provisional.
pub const MAX_LIMIT: usize = 200;

/// Any other single text field (title, name, reason, error line) is cut to
/// this many characters. Provisional. A cut plain text field keeps its first
/// `FIELD_MAX_CHARS - 1` characters and ends with `…`; a [`BoundedText`]
/// sets `truncated` instead.
///
/// [`BoundedText`]: crate::model::BoundedText
pub const FIELD_MAX_CHARS: usize = 200;

/// Applies [`FIELD_MAX_CHARS`] to a plain text field: unchanged when short
/// enough, otherwise its first `FIELD_MAX_CHARS - 1` characters and `…`.
pub fn cap_field(text: &str) -> String {
    if text.chars().count() <= FIELD_MAX_CHARS {
        return text.to_owned();
    }
    let mut kept: String = text.chars().take(FIELD_MAX_CHARS - 1).collect();
    kept.push('…');
    kept
}
