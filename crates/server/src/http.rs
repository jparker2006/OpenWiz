//! A strict, bounded HTTP/1.1 request head reader and a response writer.
//!
//! Only what the API needs: an origin-form request target, `HTTP/1.1` or
//! `HTTP/1.0`, CRLF line endings, no folded headers, no body. Anything else
//! is refused before it reaches a route.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::time::{Duration, Instant};

use crate::Limits;

/// The longest method token accepted.
const MAX_METHOD_BYTES: usize = 16;

/// A request head that passed the syntax checks.
#[derive(Debug)]
pub(crate) struct Head {
    pub method: String,
    pub target: String,
    pub headers: Vec<(String, String)>,
}

impl Head {
    /// Every value of the header `name`.
    pub fn values(&self, name: &str) -> Vec<&str> {
        self.headers
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
            .collect()
    }
}

/// Why no request head could be used.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum HeadError {
    /// The client closed the connection before sending anything.
    Closed,
    /// The head did not arrive within the head timeout.
    Timeout,
    TargetTooLong,
    TooLarge,
    Malformed,
    /// An HTTP version other than 1.1 and 1.0.
    Version,
}

/// A head that could not be used, and whether the bytes read so far may
/// be a `HEAD` request (so that the refusal is sent without a body).
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Refusal {
    pub error: HeadError,
    pub head_method: bool,
}

/// Whether `bytes`, the first bytes of a request, are or may become the
/// method `HEAD`: they start with `HEAD `, or are a non-empty beginning of
/// it (the rest has not arrived).
pub(crate) fn may_be_head(bytes: &[u8]) -> bool {
    !bytes.is_empty() && (bytes.starts_with(b"HEAD ") || b"HEAD ".starts_with(bytes))
}

/// Reads one request head within `limits`. Bytes after the head are left
/// unread (a body is refused later).
pub(crate) fn read_head(stream: &mut TcpStream, limits: &Limits) -> Result<Head, Refusal> {
    let mut buf: Vec<u8> = Vec::with_capacity(1024);
    read_head_into(stream, limits, &mut buf).map_err(|error| Refusal {
        error,
        head_method: may_be_head(&buf),
    })
}

fn read_head_into(
    stream: &mut TcpStream,
    limits: &Limits,
    buf: &mut Vec<u8>,
) -> Result<Head, HeadError> {
    let deadline = Instant::now() + limits.head_timeout;
    let max_line = limits.max_target_bytes + MAX_METHOD_BYTES + " HTTP/1.1 ".len();
    let mut chunk = [0u8; 4096];
    loop {
        if let Some(end) = find(buf, b"\r\n\r\n") {
            if has_bare_lf(&buf[..end]) {
                return Err(HeadError::Malformed);
            }
            if end + 4 > limits.max_head_bytes {
                return Err(HeadError::TooLarge);
            }
            return parse(&buf[..end], limits);
        }
        if has_bare_lf(buf) {
            return Err(HeadError::Malformed);
        }
        let line = find(buf, b"\r\n").unwrap_or(buf.len());
        if line > max_line {
            return Err(HeadError::TargetTooLong);
        }
        if buf.len() > limits.max_head_bytes {
            return Err(HeadError::TooLarge);
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(HeadError::Timeout);
        }
        stream
            .set_read_timeout(Some(left))
            .map_err(|_| HeadError::Closed)?;
        match stream.read(&mut chunk) {
            Ok(0) if buf.is_empty() => return Err(HeadError::Closed),
            Ok(0) => return Err(HeadError::Malformed),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return Err(HeadError::Timeout);
            }
            Err(_) => return Err(HeadError::Closed),
        }
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// A line feed not preceded by a carriage return.
fn has_bare_lf(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .enumerate()
        .any(|(i, &b)| b == b'\n' && (i == 0 || bytes[i - 1] != b'\r'))
}

/// RFC 9110 `tchar`.
fn is_tchar(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)
}

/// `head` is everything before the blank line, without its CRLF.
fn parse(head: &[u8], limits: &Limits) -> Result<Head, HeadError> {
    let mut lines = head.split(|&b| b == b'\n').map(|line| {
        // Every line ends in CRLF (bare LFs were refused), except the last,
        // whose CRLF is part of the blank line.
        line.strip_suffix(b"\r").unwrap_or(line)
    });
    let request_line = lines.next().ok_or(HeadError::Malformed)?;
    let parts: Vec<&[u8]> = request_line.split(|&b| b == b' ').collect();
    let [method, target, version] = parts[..] else {
        return Err(HeadError::Malformed);
    };
    if method.is_empty() || method.len() > MAX_METHOD_BYTES || !method.iter().all(|&b| is_tchar(b))
    {
        return Err(HeadError::Malformed);
    }
    if target.len() > limits.max_target_bytes {
        return Err(HeadError::TargetTooLong);
    }
    let target_ok = target.first() == Some(&b'/')
        && target
            .iter()
            .all(|&b| (0x21..=0x7e).contains(&b) && b != b'#');
    if !target_ok {
        return Err(HeadError::Malformed);
    }
    match version {
        b"HTTP/1.1" | b"HTTP/1.0" => {}
        [b'H', b'T', b'T', b'P', b'/', major, b'.', minor]
            if major.is_ascii_digit() && minor.is_ascii_digit() =>
        {
            return Err(HeadError::Version);
        }
        _ => return Err(HeadError::Malformed),
    }

    let mut headers = Vec::new();
    for line in lines {
        if headers.len() == limits.max_header_lines {
            return Err(HeadError::TooLarge);
        }
        let colon = line
            .iter()
            .position(|&b| b == b':')
            .ok_or(HeadError::Malformed)?;
        let (name, value) = (&line[..colon], &line[colon + 1..]);
        if name.is_empty() || !name.iter().all(|&b| is_tchar(b)) {
            return Err(HeadError::Malformed);
        }
        let value_ok = value
            .iter()
            .all(|&b| b == b'\t' || (0x20..=0x7e).contains(&b) || b >= 0x80);
        if !value_ok {
            return Err(HeadError::Malformed);
        }
        let value = String::from_utf8_lossy(value)
            .trim_matches([' ', '\t'])
            .to_owned();
        headers.push((String::from_utf8_lossy(name).into_owned(), value));
    }
    Ok(Head {
        // Checked above: ASCII only.
        method: String::from_utf8_lossy(method).into_owned(),
        target: String::from_utf8_lossy(target).into_owned(),
        headers,
    })
}

/// A response: status, its fixed headers ([`API_HEADERS`] or the web
/// page's), extra headers and a body.
#[derive(Clone, Debug)]
pub(crate) struct Reply {
    pub status: u16,
    pub headers: &'static [(&'static str, &'static str)],
    pub extra: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        414 => "URI Too Long",
        421 => "Misdirected Request",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        505 => "HTTP Version Not Supported",
        _ => "Unknown",
    }
}

/// The response as sent: without a body or `Content-Length` when `bare`.
fn wire(reply: &Reply, bare: bool) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {} {}\r\n", reply.status, reason(reply.status));
    for (name, value) in reply.headers {
        out.push_str(&format!("{name}: {value}\r\n"));
    }
    for (name, value) in &reply.extra {
        out.push_str(&format!("{name}: {value}\r\n"));
    }
    if bare {
        out.push_str("\r\n");
        return out.into_bytes();
    }
    out.push_str(&format!("Content-Length: {}\r\n\r\n", reply.body.len()));
    let mut out = out.into_bytes();
    out.extend_from_slice(&reply.body);
    out
}

/// Answers `reply` without ever blocking, for the accept loop: no body and
/// no `Content-Length` (the method is not known, and this suits GET and
/// HEAD alike), one non-blocking write of a few hundred bytes into an empty
/// send buffer, a half-close, then whatever the client has already sent is
/// read and dropped (at most 64 KiB), so it does not turn the close into a
/// reset. Best effort: bytes the client sends after this may still do so.
pub(crate) fn send_now(stream: &TcpStream, reply: &Reply) {
    if stream.set_nonblocking(true).is_err() {
        return;
    }
    let mut writer = stream;
    let _ = writer.write(&wire(reply, true));
    let _ = stream.shutdown(Shutdown::Write);
    let mut reader = stream;
    let mut sink = [0u8; 4096];
    let mut drained = 0usize;
    while drained < 64 * 1024 {
        match reader.read(&mut sink) {
            Ok(n) if n > 0 => drained += n,
            _ => break,
        }
    }
}

/// The headers on every API response, and every error.
pub(crate) const API_HEADERS: &[(&str, &str)] = &[
    ("Content-Type", "application/json"),
    ("Cache-Control", "no-store"),
    ("X-Content-Type-Options", "nosniff"),
    ("Referrer-Policy", "no-referrer"),
    ("X-Frame-Options", "DENY"),
    (
        "Content-Security-Policy",
        "default-src 'none'; frame-ancestors 'none'",
    ),
    ("Connection", "close"),
];

/// Writes `reply` within `write_timeout` and closes the connection. A
/// response to a `HEAD` request (`head`) has no body and no
/// `Content-Length`, which would have to describe a `GET`'s body
/// (RFC 9110, 8.6 and 9.3.2). The
/// write side first, then what the client still sends is read and dropped
/// for at most `linger`, so unread request bytes do not turn the close into
/// a reset that could destroy the response.
pub(crate) fn send(
    stream: &mut TcpStream,
    reply: &Reply,
    head: bool,
    write_timeout: Duration,
    linger: Duration,
) {
    let out = wire(reply, head);
    // One deadline for the whole response, not for each write, so a client
    // that reads slowly cannot hold the connection longer.
    let until = Instant::now() + write_timeout;
    let mut rest = &out[..];
    while !rest.is_empty() {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() || stream.set_write_timeout(Some(left)).is_err() {
            return;
        }
        match stream.write(rest) {
            Ok(0) => return,
            Ok(n) => rest = &rest[n..],
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return,
        }
    }
    let _ = stream.shutdown(Shutdown::Write);
    let until = Instant::now() + linger;
    let mut sink = [0u8; 4096];
    let mut drained = 0usize;
    while drained < 64 * 1024 {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() || stream.set_read_timeout(Some(left)).is_err() {
            break;
        }
        match stream.read(&mut sink) {
            Ok(0) | Err(_) => break,
            Ok(n) => drained += n,
        }
    }
}
