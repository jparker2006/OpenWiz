//! AW3: uploads are bounded. A body over 25 MiB is
//! refused before a byte of it is read; a missing, repeated or lying
//! `Content-Length`, a body slower than the upload deadline, a third upload
//! while two run, low disk and a bad name are each refused. **None of them
//! leaves anything staged: no attachment, and no `.partial`.** Nothing of
//! an upload is logged but its method, route, status and time.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::Arc;
use std::time::{Duration, Instant};

use openwiz_core::attachment::FREE_DISK_FLOOR_BYTES;
use openwiz_server::{
    Limits, MAX_NAME_BYTES, MAX_UPLOAD_BYTES, MAX_UPLOADS, NAME_HEADER, UPLOAD_DEADLINE,
};

use crate::attach_support::{
    ATTACHMENTS, Free, State, Unmeasurable, encode, list, ready_id, serve_attach, upload,
    upload_headers, upload_with,
};
use crate::http::{Response, parse_response, read_response};
use crate::send_support::{raw, with, without};

#[test]
fn the_approved_bounds() {
    assert_eq!(MAX_UPLOAD_BYTES, 26_214_400, "25 MiB (B-9)");
    assert_eq!(MAX_UPLOADS, 2, "two at a time (B-9)");
    assert_eq!(
        UPLOAD_DEADLINE,
        Duration::from_secs(110),
        "under Cloudflare's 125 s (B-9)"
    );
    assert_eq!(MAX_NAME_BYTES, 1024, "the header's cap");
    assert_eq!(NAME_HEADER, "X-OpenWiz-Name");
    let limits = Limits::default();
    assert_eq!(limits.max_uploads, MAX_UPLOADS);
    assert_eq!(limits.max_upload_bytes, MAX_UPLOAD_BYTES);
    assert_eq!(limits.upload_deadline, UPLOAD_DEADLINE);
}

/// The head of an upload of `len` declared bytes, without its body.
fn head(name: &str, len: u64) -> Vec<u8> {
    let mut headers = upload_headers(name);
    headers.push(("Content-Length".to_owned(), len.to_string()));
    raw("POST", ATTACHMENTS, &headers, b"", false)
}

fn connect(s: &crate::http::TestServer) -> TcpStream {
    let stream = TcpStream::connect(s.addr).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    stream
}

#[test]
fn a_body_over_the_cap_is_refused_before_it_is_read() {
    let state = State::new("web_aw3_cap");
    let s = serve_attach(&state, Free::plenty(), Limits::default());
    for len in [MAX_UPLOAD_BYTES + 1, u64::MAX, 99_999_999_999_999_999] {
        // Only the head is sent: the answer comes without waiting for a body.
        let mut stream = connect(&s);
        stream.write_all(&head("aw3.bin", len)).unwrap();
        let r = read_response(&mut stream);
        assert_eq!(r.status, 413, "{len}: {}", r.text());
        assert_eq!(r.error().0, "body_too_large");
    }
    let mut digits = upload_headers("aw3.bin");
    digits.push(("Content-Length".to_owned(), "9".repeat(40)));
    let r = s.send(&raw("POST", ATTACHMENTS, &digits, b"", false));
    assert_eq!(r.status, 413, "{}", r.text());
    assert!(state.nothing_staged());
}

#[test]
fn a_body_at_the_cap_is_staged() {
    let state = State::new("web_aw3_at_cap");
    let s = serve_attach(&state, Free::plenty(), Limits::default());
    let bytes = vec![b'a'; usize::try_from(MAX_UPLOAD_BYTES).unwrap()];
    let r = upload(&s, "aw3-at-cap.txt", &bytes);
    let id = ready_id(&r);
    assert_eq!(
        r.json()["ready"][0]["staged"]["attachment"]["size_bytes"],
        MAX_UPLOAD_BYTES
    );
    assert!(id.starts_with("att-"));
}

#[test]
fn the_framing_is_refused_without_a_length_or_with_two() {
    let state = State::new("web_aw3_framing");
    let s = serve_attach(&state, Free::plenty(), Limits::default());
    let body = b"Synthetic AW3 text\n";
    // No Content-Length.
    let r = s.send(&raw(
        "POST",
        ATTACHMENTS,
        &upload_headers("aw3.txt"),
        body,
        false,
    ));
    assert_eq!(r.status, 411, "{}", r.text());
    assert_eq!(r.error().0, "length_required");
    for (what, extra) in [
        ("chunked", vec![("Transfer-Encoding", "chunked")]),
        (
            "two lengths",
            vec![("Content-Length", "19"), ("Content-Length", "19")],
        ),
        ("a signed length", vec![("Content-Length", "+19")]),
        ("a spaced length", vec![("Content-Length", "1 9")]),
        ("an empty length", vec![("Content-Length", "")]),
    ] {
        let mut headers = upload_headers("aw3.txt");
        headers.extend(
            extra
                .iter()
                .map(|(n, v)| ((*n).to_owned(), (*v).to_owned())),
        );
        let r = s.send(&raw("POST", ATTACHMENTS, &headers, body, false));
        assert_eq!(r.status, 400, "{what}: {}", r.text());
        assert_eq!(r.error().0, "bad_request", "{what}");
    }
    assert!(state.nothing_staged());
}

#[test]
fn a_body_shorter_than_its_length_stages_nothing() {
    let state = State::new("web_aw3_short");
    let s = serve_attach(&state, Free::plenty(), Limits::default());
    for (declared, sent) in [(4096_u64, 1000_usize), (100_000, 70_000), (10, 0)] {
        let mut stream = connect(&s);
        stream.write_all(&head("aw3-short.txt", declared)).unwrap();
        stream.write_all(&vec![b'b'; sent]).unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        // Whatever the answer, it is a refusal, and it is never ready.
        let mut bytes = vec![];
        let _ = stream.read_to_end(&mut bytes);
        if !bytes.is_empty() {
            let r = parse_response(&bytes);
            assert_eq!(r.status, 400, "{declared}/{sent}: {}", r.text());
        }
        s.wait_for("the upload to end", |st| st.connections == 0);
        assert!(
            state.nothing_staged(),
            "{declared}/{sent}: {:?}",
            state.entries()
        );
    }
}

#[test]
fn a_body_longer_than_its_length_is_read_only_to_its_length() {
    let state = State::new("web_aw3_long");
    let s = serve_attach(&state, Free::plenty(), Limits::default());
    let mut headers = upload_headers("aw3-long.txt");
    headers.push(("Content-Length".to_owned(), "100".to_owned()));
    let mut request = raw("POST", ATTACHMENTS, &headers, b"", false);
    request.extend(vec![b'c'; 100]);
    request.extend(b"GET /api/v1/health HTTP/1.1\r\nHost: openwiz.example\r\n\r\n");
    let r = s.send(&request);
    ready_id(&r);
    assert_eq!(
        r.json()["ready"][0]["staged"]["attachment"]["size_bytes"],
        100
    );
    assert_eq!(state.entries().len(), 1);
}

#[test]
fn a_body_slower_than_the_deadline_is_refused_and_stages_nothing() {
    let state = State::new("web_aw3_slow");
    let limits = Limits {
        upload_deadline: Duration::from_secs(1),
        ..Limits::default()
    };
    let s = serve_attach(&state, Free::plenty(), limits);
    // Enough bytes for the store to start its copy, then nothing.
    let mut stream = connect(&s);
    stream.write_all(&head("aw3-slow.txt", 100_000)).unwrap();
    stream.write_all(&vec![b'd'; 10_000]).unwrap();
    let started = Instant::now();
    let r = read_response(&mut stream);
    assert_eq!(r.status, 408, "{}", r.text());
    assert_eq!(r.error().0, "request_timeout");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
    drop(stream);
    s.wait_for("the upload to end", |st| st.connections == 0);
    assert!(state.nothing_staged(), "{:?}", state.entries());
}

/// An upload left open with `sent` of `declared` bytes.
fn hold(s: &crate::http::TestServer, name: &str, declared: u64, sent: usize) -> TcpStream {
    let mut stream = connect(s);
    stream.write_all(&head(name, declared)).unwrap();
    stream.write_all(&vec![b'e'; sent]).unwrap();
    stream
}

#[test]
fn a_third_upload_while_two_run_is_busy_and_stages_nothing() {
    let state = State::new("web_aw3_busy");
    let limits = Limits {
        slot_wait: Duration::from_millis(300),
        ..Limits::default()
    };
    let s = serve_attach(&state, Free::plenty(), limits);
    let held: Vec<TcpStream> = (0..MAX_UPLOADS)
        .map(|n| hold(&s, &format!("aw3-held-{n}.txt"), 1_000_000, 70_000))
        .collect();
    // Both hold their slots.
    s.wait_for("the held uploads to take both slots", |st| {
        st.uploads == MAX_UPLOADS
    });
    let r = upload(&s, "aw3-third.txt", b"Synthetic AW3 third\n");
    assert_eq!(r.status, 503, "{}", r.text());
    assert_eq!(r.error().0, "busy");
    assert_eq!(r.header("Retry-After"), Some("5"));
    // The held two stream into the store's unfinished copies; the third
    // left nothing there.
    let until = Instant::now() + Duration::from_secs(10);
    let partials = loop {
        let mut names: Vec<String> = state
            .partials()
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        names.sort();
        if names.len() == MAX_UPLOADS || Instant::now() > until {
            break names;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(
        partials,
        ["aw3-held-0.txt.partial", "aw3-held-1.txt.partial"],
        "the third staged nothing"
    );
    assert_eq!(state.entries().len(), MAX_UPLOADS);
    // The list and the other routes are not held up by uploads.
    let r = list(&s);
    assert_eq!(r.status, 200, "{}", r.text());
    // Cut off, the held uploads leave nothing behind.
    drop(held);
    s.wait_for("the held uploads to end", |st| st.connections == 0);
    assert!(state.nothing_staged(), "{:?}", state.entries());
    // And a slot is free again.
    ready_id(&upload(&s, "aw3-after.txt", b"Synthetic AW3 after\n"));
}

#[test]
fn low_disk_fails_with_the_cores_reason_and_stages_nothing() {
    let state = State::new("web_aw3_low_disk");
    let free = Free::plenty();
    let s = serve_attach(&state, free.clone(), Limits::default());
    free.set(FREE_DISK_FLOOR_BYTES);
    let r = upload(&s, "aw3-low.txt", b"Synthetic AW3 low disk\n");
    assert_eq!(r.status, 200, "{}", r.text());
    let body = r.json();
    assert_eq!(body["ready"], serde_json::json!([]));
    assert_eq!(body["failed"][0]["readiness"]["state"], "failed");
    assert_eq!(body["failed"][0]["readiness"]["reason"], "low_disk");
    assert!(state.nothing_staged(), "{:?}", state.entries());
}

#[test]
fn an_unmeasurable_disk_is_a_store_failure_and_stages_nothing() {
    let state = State::new("web_aw3_unmeasured");
    let s = serve_attach(&state, Arc::new(Unmeasurable), Limits::default());
    let r = upload(&s, "aw3-unmeasured.txt", b"Synthetic AW3\n");
    assert_eq!(r.status, 500, "{}", r.text());
    let (kind, message) = r.error();
    assert_eq!(kind, "store_unavailable");
    assert!(!message.contains('/'), "no path: {message}");
    assert!(state.nothing_staged(), "{:?}", state.entries());
}

#[test]
fn the_name_header_is_one_percent_encoded_utf8_name() {
    let state = State::new("web_aw3_name");
    let s = serve_attach(&state, Free::plenty(), Limits::default());
    let body = b"Synthetic AW3 name\n";
    let good = upload_headers("aw3.txt");
    let refused: Vec<(&str, Vec<(String, String)>)> = vec![
        ("no name", without(&good, NAME_HEADER)),
        ("an empty name", with(&good, NAME_HEADER, "")),
        ("a raw space", with(&good, NAME_HEADER, "a b.txt")),
        ("a raw slash", with(&good, NAME_HEADER, "a/b.txt")),
        ("a bad escape", with(&good, NAME_HEADER, "a%2.txt")),
        ("a non-hex escape", with(&good, NAME_HEADER, "a%zz.txt")),
        ("invalid UTF-8", with(&good, NAME_HEADER, "a%FF.txt")),
        ("an encoded NUL", with(&good, NAME_HEADER, "a%00.txt")),
        ("an encoded slash", with(&good, NAME_HEADER, "a%2Fb.txt")),
        ("a dot", with(&good, NAME_HEADER, ".")),
        ("two dots", with(&good, NAME_HEADER, "..")),
        ("two encoded dots", with(&good, NAME_HEADER, "%2E%2E")),
        (
            "one byte too long",
            with(&good, NAME_HEADER, &"a".repeat(MAX_NAME_BYTES + 1)),
        ),
        ("two names", {
            let mut h = good.clone();
            h.push((NAME_HEADER.to_owned(), "b.txt".to_owned()));
            h
        }),
    ];
    for (what, headers) in refused {
        let r = upload_with(&s, &headers, body);
        assert_eq!(r.status, 400, "{what}: {}", r.text());
        assert_eq!(r.error().0, "invalid_request", "{what}");
        assert!(state.nothing_staged(), "{what}");
    }
    // At the bound, and a name that is all escapes, are names.
    for name in [
        "a".repeat(MAX_NAME_BYTES - 4) + ".txt",
        "Synthétique résumé (1).txt".to_owned(),
    ] {
        let r = upload_with(&s, &with(&good, NAME_HEADER, &encode(&name)), body);
        ready_id(&r);
        assert_eq!(
            r.json()["ready"][0]["staged"]["attachment"]["display_name"],
            name.as_str()
        );
    }
}

#[test]
fn nothing_of_an_upload_is_logged() {
    let state = State::new("web_aw3_log");
    let s = serve_attach(&state, Free::plenty(), Limits::default());
    let name = "aw3-secret-name.txt";
    let r = upload(&s, name, b"Synthetic AW3 log body\n");
    let id = ready_id(&r);
    let sha = r.json()["ready"][0]["staged"]["attachment"]["sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let _ = list(&s);
    let _ = crate::attach_support::remove(&s, &id);
    let log = s.settled_log();
    assert_eq!(log.lines().count(), 3, "{log}");
    for line in log.lines() {
        let words: Vec<&str> = line.split(' ').collect();
        assert_eq!(words.len(), 4, "{line}");
        assert!(["POST", "GET", "DELETE"].contains(&words[0]), "{line}");
        assert!(
            ["/api/v1/attachments", "/api/v1/attachments/{id}"].contains(&words[1]),
            "{line}"
        );
        assert!(
            words[2].len() == 3 && words[2].bytes().all(|b| b.is_ascii_digit()),
            "{line}"
        );
        let ms = words[3].strip_suffix("ms").expect("a time");
        assert!(ms.bytes().all(|b| b.is_ascii_digit()), "{line}");
    }
    for secret in [name, id.as_str(), sha.as_str(), "Synthetic AW3 log body"] {
        assert!(!log.contains(secret), "the log carries {secret}: {log}");
    }
}

/// The answer to a refused upload whose body is still coming can be read:
/// the server reads and discards it, up to its length, first.
#[test]
fn a_refusal_mid_body_can_still_be_read() {
    let state = State::new("web_aw3_drain");
    let free = Free::plenty();
    let s = serve_attach(&state, free.clone(), Limits::default());
    free.set(0);
    let body = vec![b'f'; 3 * 1024 * 1024];
    let r: Response = upload(&s, "aw3-drain.txt", &body);
    assert_eq!(r.status, 200, "{}", r.status_line);
    assert_eq!(r.json()["failed"][0]["readiness"]["reason"], "low_disk");
    assert!(state.nothing_staged());
}
