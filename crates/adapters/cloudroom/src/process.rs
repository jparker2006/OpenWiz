//! Runs one allowlisted `room-cli` read under a deadline.
//!
//! Standard library only: a child process, two reader threads and polling.
//! A read is stopped at the earlier of the run's deadline and
//! [`READ_TIMEOUT`]; a stopped child is killed and reaped. Nothing is retried.
//!
//! Only the child itself is killed. A process it started that keeps the
//! pipes open is not; the read then times out without waiting longer.
//! Whether `room-cli` starts such processes is unobserved.

use std::io::Read as _;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use openwiz_core::limits::READ_TIMEOUT;
use openwiz_core::source::SourceError;

use crate::allowlist::Invocation;

/// The most stdout a read may print. `thread list` was observed to print
/// about 1.2 MB for several hundred sessions. Provisional.
pub const MAX_STDOUT_BYTES: usize = 32 * 1024 * 1024;

/// The most stderr kept from a read; only its first line is used, through
/// [`diagnostic`].
const MAX_STDERR_BYTES: usize = 64 * 1024;

/// How long the output of a child that has exited may still be collected
/// after the deadline.
const DRAIN_GRACE: Duration = Duration::from_millis(100);

/// How often a running read is checked.
const POLL: Duration = Duration::from_millis(5);

/// Runs `program` with exactly `invocation.argv()` and returns its stdout.
pub fn run(
    program: &Path,
    invocation: &Invocation,
    deadline: Instant,
) -> Result<Vec<u8>, SourceError> {
    let started = Instant::now();
    let stop_at = deadline.min(started + READ_TIMEOUT);
    if started >= stop_at {
        return Err(SourceError::TimedOut { after_ms: 0 });
    }
    let mut child = Command::new(program)
        .args(invocation.argv())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| SourceError::Unavailable {
            detail: format!("{} could not be started: {e}", program.display()),
        })?;
    let stdout = collect(child.stdout.take(), MAX_STDOUT_BYTES);
    let stderr = collect(child.stderr.take(), MAX_STDERR_BYTES);
    let timed_out = || SourceError::TimedOut {
        after_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    };

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                let left = stop_at.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    stop(&mut child);
                    return Err(timed_out());
                }
                thread::sleep(POLL.min(left));
            }
            Err(e) => {
                stop(&mut child);
                return Err(SourceError::Unavailable {
                    detail: format!("waiting for {} failed: {e}", program.display()),
                });
            }
        }
    };

    // The pipes close when the child exits, unless something it started
    // still holds them; the deadline, plus a short grace for a child that
    // exited just before it, bounds that wait too.
    let drain_until = stop_at.max(Instant::now() + DRAIN_GRACE);
    let left = drain_until.saturating_duration_since(Instant::now());
    let out = stdout.recv_timeout(left).map_err(|_| timed_out())?;
    let left = drain_until.saturating_duration_since(Instant::now());
    let err = stderr.recv_timeout(left).unwrap_or_default();

    if out.len() > MAX_STDOUT_BYTES {
        return Err(SourceError::Malformed {
            detail: format!("printed more than {MAX_STDOUT_BYTES} bytes"),
        });
    }
    if status.success() {
        return Ok(out);
    }
    Err(SourceError::CommandFailed {
        exit_code: status.code(),
        detail: diagnostic(&err),
    })
}

/// The longest reason phrase kept after `Error: HTTP <code>: `.
const MAX_REASON_CHARS: usize = 60;

/// The not-found line observed in capture C. A first line that is exactly
/// this line is kept; one that only contains its `HTTP 404: Thread not
/// found` is reported as this line and ` (detail withheld)`. So `thread
/// show` and `thread output` (for `show` and `ls -l`) still find not-found by
/// prefix, as before, and nothing else of the line is shown.
pub const NOT_FOUND_LINE: &str = "Error: HTTP 404: Thread not found";

/// The line shown for a failed read: `room-cli`'s first non-empty stderr
/// line, only if it is in the safe grammar, `Error: HTTP <code>` and
/// optionally `: ` and a reason phrase of ASCII letters and single spaces.
/// Otherwise only its status is kept, as `Error: HTTP <code> (detail
/// withheld)`, or nothing of it. No digit beyond the status, punctuation,
/// control or non-ASCII character from `room-cli` is shown, so no path,
/// address, `KEY=value` or terminal control; a word of letters still can
/// be. Provisional.
fn diagnostic(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let Some(line) = text.lines().map(str::trim).find(|l| !l.is_empty()) else {
        return "no error output".to_owned();
    };
    if line != NOT_FOUND_LINE && line.contains("HTTP 404: Thread not found") {
        return format!("{NOT_FOUND_LINE} (detail withheld)");
    }
    if is_safe(line) {
        return line.to_owned();
    }
    match http_status(line) {
        Some(code) => format!("Error: HTTP {code} (detail withheld)"),
        None => "room-cli error (detail withheld)".to_owned(),
    }
}

fn is_safe(line: &str) -> bool {
    let Some((code, reason)) = line
        .strip_prefix("Error: HTTP ")
        .and_then(|rest| rest.split_at_checked(3))
    else {
        return false;
    };
    is_status(code)
        && (reason.is_empty()
            || reason.strip_prefix(": ").is_some_and(|phrase| {
                phrase.chars().count() <= MAX_REASON_CHARS
                    && phrase
                        .split(' ')
                        .all(|w| !w.is_empty() && w.bytes().all(|b| b.is_ascii_alphabetic()))
            }))
}

/// The first `HTTP <code>` in `line` that is a word of its own.
fn http_status(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    line.match_indices("HTTP ").find_map(|(i, _)| {
        let code = line.get(i + 5..i + 8)?;
        let starts_word = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
        let ends_word = !bytes.get(i + 8).is_some_and(u8::is_ascii_digit);
        (starts_word && ends_word && is_status(code)).then_some(code)
    })
}

/// Three ASCII digits from 100 to 599.
fn is_status(code: &str) -> bool {
    let b = code.as_bytes();
    b.len() == 3 && (b'1'..=b'5').contains(&b[0]) && b[1..].iter().all(u8::is_ascii_digit)
}

/// Reads a pipe to its end, or to `limit + 1` bytes, on its own thread.
fn collect(pipe: Option<impl std::io::Read + Send + 'static>, limit: usize) -> Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(pipe) = pipe {
            let _ = pipe.take(limit as u64 + 1).read_to_end(&mut buf);
        }
        let _ = tx.send(buf);
    });
    rx
}

/// Kills the child and reaps it, so no process is left behind.
fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}
