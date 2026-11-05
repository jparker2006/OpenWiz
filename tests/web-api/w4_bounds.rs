//! W4: the send route is bounded: one target from the path,
//! a capped body with `Content-Length`, strict JSON, a cap on sends at
//! once; every answer is `no-store`; and the text, op id and target never
//! reach the log.

use std::thread;
use std::time::Duration;

use openwiz_server::{Limits, MAX_SEND_BODY_BYTES, MAX_SENDS};
use serde_json::json;

use crate::http::Response;
use crate::msg::{self, SendFake};
use crate::send_support::{
    body, good_headers, messages, post, raw, send_json, serve_send, serve_send_limits, with,
};

fn refused_body(fake: &SendFake, what: &str, r: &Response, status: u16, kind: &str) {
    assert_eq!(r.status, status, "{what}: {}", r.text());
    assert_eq!(r.error().0, kind, "{what}");
    assert_eq!(r.header("Cache-Control"), Some("no-store"), "{what}");
    assert_eq!(fake.tell_count(), 0, "{what}: a tell was made");
    assert!(
        fake.invocations().is_empty(),
        "{what}: {:?}",
        fake.invocations()
    );
}

#[test]
fn the_send_bounds_are_the_documented_values() {
    assert_eq!(MAX_SENDS, 2);
    assert_eq!(MAX_SEND_BODY_BYTES, 99_328);
    assert_eq!(openwiz_server::BODY_TIMEOUT, Duration::from_secs(10));
}

#[test]
fn a_body_over_the_cap_is_refused_unread_and_one_at_the_cap_is_read() {
    let fake = SendFake::new("web_w4_cap", "send");
    let s = serve_send(&fake);
    // Over: only the head is sent; the server must answer without the body.
    let mut head = format!("POST {} HTTP/1.1\r\n", messages("thr_syn_0001"));
    for (n, v) in good_headers() {
        head.push_str(&format!("{n}: {v}\r\n"));
    }
    head.push_str(&format!(
        "Content-Length: {}\r\n\r\n",
        MAX_SEND_BODY_BYTES + 1
    ));
    let r = s.send(head.as_bytes());
    refused_body(&fake, "one byte over", &r, 413, "body_too_large");
    // At the cap: a valid object padded with whitespace is a send.
    let mut payload = body("w4-op-cap", "queue", "Synthetic W4 text").to_string();
    payload.push_str(&" ".repeat(MAX_SEND_BODY_BYTES - payload.len()));
    assert_eq!(payload.len(), MAX_SEND_BODY_BYTES);
    let r = post(&s, "thr_syn_0001", &good_headers(), payload.as_bytes());
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(fake.tell_count(), 1);
}

#[test]
fn a_body_that_breaks_the_rules_is_refused_with_no_call() {
    let fake = SendFake::new("web_w4_rules", "send");
    let s = serve_send(&fake);
    let good = r#"{"op_id":"w4-op-1","mode":"queue","text":"Synthetic W4 text"}"#;
    let cases: Vec<(&str, Vec<u8>)> =
        vec![
        (
            "a target key",
            br#"{"op_id":"w4-op-1","mode":"queue","text":"t","target":"thr_syn_0002"}"#.to_vec(),
        ),
        ("an unknown key", br#"{"op_id":"w4-op-1","mode":"queue","text":"t","x":1}"#.to_vec()),
        (
            "a repeated key",
            br#"{"op_id":"w4-op-1","mode":"queue","text":"t","text":"u"}"#.to_vec(),
        ),
        ("trailing data", format!("{good}{good}").into_bytes()),
        ("trailing junk", format!("{good} x").into_bytes()),
        ("malformed JSON", br#"{"op_id":"w4-op-1","mode":"queue","text":}"#.to_vec()),
        ("an array root", format!("[{good}]").into_bytes()),
        ("a string root", br#""Synthetic""#.to_vec()),
        ("an empty body", Vec::new()),
        ("no op id", br#"{"mode":"queue","text":"t"}"#.to_vec()),
        ("no mode", br#"{"op_id":"w4-op-1","text":"t"}"#.to_vec()),
        ("no text", br#"{"op_id":"w4-op-1","mode":"queue"}"#.to_vec()),
        ("mode auto", br#"{"op_id":"w4-op-1","mode":"auto","text":"t"}"#.to_vec()),
        ("mode Queue", br#"{"op_id":"w4-op-1","mode":"Queue","text":"t"}"#.to_vec()),
        ("an op id number", br#"{"op_id":7,"mode":"queue","text":"t"}"#.to_vec()),
        ("a text array", br#"{"op_id":"w4-op-1","mode":"queue","text":["t"]}"#.to_vec()),
        (
            "a flag as a string",
            br#"{"op_id":"w4-op-1","mode":"queue","text":"t","resume_paused_queue":"true"}"#
                .to_vec(),
        ),
        (
            "a flag as null",
            br#"{"op_id":"w4-op-1","mode":"queue","text":"t","accept_possible_duplicate":null}"#
                .to_vec(),
        ),
        (
            "not UTF-8",
            b"{\"op_id\":\"w4-op-1\",\"mode\":\"queue\",\"text\":\"\xff\xfe\"}".to_vec(),
        ),
    ];
    for (what, payload) in cases {
        let r = post(&s, "thr_syn_0001", &good_headers(), &payload);
        refused_body(&fake, what, &r, 400, "invalid_request");
        assert!(!fake.record_path("w4-op-1").exists(), "{what}");
    }
    // The core's own rules: 400 with the core's message, never the value.
    for (what, value, message) in [
        (
            "a bad op id",
            body("-w4", "queue", "Synthetic W4 text"),
            "an op id is 1 to 64 ASCII letters, digits, _ or -, starting with a letter or digit",
        ),
        (
            "whitespace only",
            body("w4-op-2", "queue", "  \n "),
            "the text is only whitespace",
        ),
        (
            "an empty text",
            body("w4-op-2", "queue", ""),
            "the text is empty",
        ),
        (
            "a NUL",
            body("w4-op-2", "queue", "Synthetic\u{0}W4"),
            "the text contains a NUL character",
        ),
    ] {
        let r = send_json(&s, "thr_syn_0001", &value);
        refused_body(&fake, what, &r, 400, "invalid_request");
        assert_eq!(r.error().1, message, "{what}");
    }
    let r = send_json(&s, "-thr", &body("w4-op-2", "queue", "Synthetic W4 text"));
    refused_body(&fake, "a bad target id", &r, 400, "invalid_request");
    assert_eq!(
        r.error().1,
        "a session id is 1 to 128 ASCII letters, digits, _ or -, not starting with -"
    );
}

#[test]
fn a_second_target_is_impossible() {
    let fake = SendFake::new("web_w4_targets", "send");
    let s = serve_send(&fake);
    let payload = body("w4-op-3", "queue", "Synthetic W4 text").to_string();
    for (target, status) in [
        ("/api/v1/sessions/thr_syn_0001/thr_syn_0002/messages", 405),
        ("/api/v1/sessions/thr_syn_0001,thr_syn_0002/messages", 400),
        ("/api/v1/sessions//messages", 405),
    ] {
        let r = s.send(&raw(
            "POST",
            target,
            &good_headers(),
            payload.as_bytes(),
            true,
        ));
        assert_eq!(r.status, status, "{target}: {}", r.text());
    }
    assert_eq!(fake.tell_count(), 0);
}

#[test]
fn framing_is_strict() {
    let fake = SendFake::new("web_w4_framing", "send");
    let s = serve_send_limits(
        &fake,
        Limits {
            body_timeout: Duration::from_millis(500),
            ..Limits::default()
        },
    );
    let payload = body("w4-op-4", "queue", "Synthetic W4 text").to_string();
    let target = messages("thr_syn_0001");
    // No Content-Length.
    let r = s.send(&raw(
        "POST",
        &target,
        &good_headers(),
        payload.as_bytes(),
        false,
    ));
    refused_body(&fake, "no length", &r, 411, "length_required");
    // Chunked.
    let chunked = with(&good_headers(), "Transfer-Encoding", "chunked");
    let r = s.send(&raw("POST", &target, &chunked, b"0\r\n\r\n", false));
    refused_body(&fake, "chunked", &r, 400, "bad_request");
    // Both.
    let r = s.send(&raw("POST", &target, &chunked, payload.as_bytes(), true));
    refused_body(&fake, "chunked and a length", &r, 400, "bad_request");
    // Two lengths, a signed length, a hex length.
    for (what, value) in [
        ("two lengths", None),
        ("a signed length", Some("+5")),
        ("a hex length", Some("0x10")),
        ("an empty length", Some("")),
    ] {
        let mut headers = good_headers();
        match value {
            None => {
                headers.push(("Content-Length".to_owned(), payload.len().to_string()));
                headers.push(("Content-Length".to_owned(), payload.len().to_string()));
            }
            Some(v) => headers.push(("Content-Length".to_owned(), v.to_owned())),
        }
        let r = s.send(&raw("POST", &target, &headers, payload.as_bytes(), false));
        refused_body(&fake, what, &r, 400, "bad_request");
    }
    // A body shorter than its length: 408 when it does not arrive.
    let mut headers = good_headers();
    headers.push((
        "Content-Length".to_owned(),
        (payload.len() + 10).to_string(),
    ));
    let mut stream = std::net::TcpStream::connect(s.addr).unwrap();
    use std::io::Write as _;
    stream
        .write_all(&raw("POST", &target, &headers, payload.as_bytes(), false))
        .unwrap();
    let r = crate::http::read_response(&mut stream);
    refused_body(&fake, "a short body", &r, 408, "request_timeout");
    // A query.
    let r = s.send(&raw(
        "POST",
        &format!("{target}?x=1"),
        &good_headers(),
        payload.as_bytes(),
        true,
    ));
    refused_body(&fake, "a query", &r, 400, "invalid_request");
    assert!(!fake.record_path("w4-op-4").exists());
}

#[test]
fn sends_over_the_cap_are_refused_before_any_claim() {
    let fake = SendFake::with(
        "web_w4_busy",
        vec![msg::tell_response(
            "thr_syn_0001",
            "queue",
            json!({ "delay_ms": 1500, "stdout_file": msg::send_fixtures().join("tell-sent.json").to_str().unwrap() }),
        )],
    );
    let s = serve_send_limits(
        &fake,
        Limits {
            max_sends: 1,
            slot_wait: Duration::from_millis(200),
            ..Limits::default()
        },
    );
    let first = {
        let addr = s.addr;
        thread::spawn(move || {
            crate::http::send(
                addr,
                &raw(
                    "POST",
                    &messages("thr_syn_0001"),
                    &good_headers(),
                    body("w4-op-5", "queue", "Synthetic W4 first")
                        .to_string()
                        .as_bytes(),
                    true,
                ),
            )
        })
    };
    // Wait until the first send is at its call.
    let until = std::time::Instant::now() + Duration::from_secs(10);
    while fake.tell_count() == 0 {
        assert!(
            std::time::Instant::now() < until,
            "the first send never called"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let r = send_json(
        &s,
        "thr_syn_0001",
        &body("w4-op-6", "queue", "Synthetic W4 second"),
    );
    assert_eq!(r.status, 503, "{}", r.text());
    assert_eq!(r.error().0, "busy");
    assert_eq!(r.header("Retry-After"), Some("5"));
    assert!(
        !fake.record_path("w4-op-6").exists(),
        "the refused send was claimed"
    );
    let r = first.join().unwrap();
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(fake.tell_count(), 1);
    // Reads are not starved by sends: their slots are separate.
    let r = crate::send_support::get_public(&s, "/api/v1/sessions/thr_syn_0001/preview");
    assert_eq!(r.status, 200, "{}", r.text());
}

#[test]
fn nothing_of_a_send_reaches_the_log() {
    let fake = SendFake::new("web_w4_log", "send");
    let s = serve_send(&fake);
    let marker = "Zq7Marker";
    let text = format!("Synthetic {marker} text");
    let op = format!("w4-{marker}");
    // An accepted send, a replay, a refusal before the core, a malformed
    // body, an oversize body, a core refusal, a not-found target, a
    // preview and a lookup.
    let _ = send_json(&s, "thr_syn_0001", &body(&op, "queue", &text));
    let _ = send_json(&s, "thr_syn_0001", &body(&op, "queue", &text));
    let _ = post(
        &s,
        "thr_syn_0001",
        &with(
            &good_headers(),
            "Origin",
            &format!("https://{marker}.example"),
        ),
        body(&op, "queue", &text).to_string().as_bytes(),
    );
    let _ = post(
        &s,
        "thr_syn_0001",
        &good_headers(),
        format!("{{\"{marker}\"").as_bytes(),
    );
    let mut big = body(&op, "queue", &text).to_string();
    big.push_str(&" ".repeat(MAX_SEND_BODY_BYTES));
    let _ = post(&s, "thr_syn_0001", &good_headers(), big.as_bytes());
    let _ = send_json(
        &s,
        "thr_syn_0001",
        &body(&op, "queue", &format!("{marker}\u{0}")),
    );
    let _ = send_json(
        &s,
        &format!("thr_syn_{marker}"),
        &body("w4-op-7", "queue", &text),
    );
    let _ =
        crate::send_support::get_public(&s, &format!("/api/v1/sessions/thr_syn_{marker}/preview"));
    let _ = crate::send_support::get_public(
        &s,
        &format!("/api/v1/sessions/thr_syn_0001/messages/{op}"),
    );
    let log = s.settled_log();
    assert!(!log.contains(marker), "the log carries the marker:\n{log}");
    assert!(
        !log.contains("thr_syn_0001"),
        "the log carries the target:\n{log}"
    );
    assert!(
        log.contains("POST /api/v1/sessions/{id}/messages 200 "),
        "{log}"
    );
    assert!(
        log.contains("GET /api/v1/sessions/{id}/preview 200 "),
        "{log}"
    );
    assert!(
        log.contains("GET /api/v1/sessions/{id}/messages/{op_id} 200 "),
        "{log}"
    );
    // The answers never repeat the text either, except where the CLI's own
    // JSON does (it never does: only its hash and byte count).
    assert_eq!(fake.tell_count(), 1);
}

#[test]
fn only_the_allowed_object_shape_is_a_body() {
    let fake = SendFake::new("web_w4_shapes", "send");
    let s = serve_send(&fake);
    let cases: Vec<(&str, &[u8])> = vec![
        (
            "a positional array of five",
            br#"["w4-op-p","queue","Synthetic W4 array",false,false]"#,
        ),
        ("a positional array of three", br#"["w4-op-p","queue","Synthetic W4 array"]"#),
        ("a positional array of one", br#"["w4-op-p"]"#),
        ("an empty array", b"[]"),
        ("an empty object", b"{}"),
        ("null", b"null"),
        ("a number", b"7"),
        ("true", b"true"),
        (
            "mode as a tagged unit",
            br#"{"op_id":"w4-op-p","mode":{"queue":null},"text":"t"}"#,
        ),
        (
            "mode as a tagged array",
            br#"{"op_id":"w4-op-p","mode":{"steer":[]},"text":"t"}"#,
        ),
        ("mode as an array", br#"{"op_id":"w4-op-p","mode":["queue"],"text":"t"}"#),
        ("mode as a number", br#"{"op_id":"w4-op-p","mode":0,"text":"t"}"#),
        ("mode null", br#"{"op_id":"w4-op-p","mode":null,"text":"t"}"#),
        ("op id null", br#"{"op_id":null,"mode":"queue","text":"t"}"#),
        ("op id an object", br#"{"op_id":{"v":"w4-op-p"},"mode":"queue","text":"t"}"#),
        ("text a number", br#"{"op_id":"w4-op-p","mode":"queue","text":7}"#),
        ("text null", br#"{"op_id":"w4-op-p","mode":"queue","text":null}"#),
        ("text an object", br#"{"op_id":"w4-op-p","mode":"queue","text":{"t":"t"}}"#),
        (
            "a flag as a number",
            br#"{"op_id":"w4-op-p","mode":"queue","text":"t","resume_paused_queue":1}"#,
        ),
        (
            "a flag as an object",
            br#"{"op_id":"w4-op-p","mode":"queue","text":"t","accept_possible_duplicate":{}}"#,
        ),
        (
            "a repeated flag",
            br#"{"op_id":"w4-op-p","mode":"queue","text":"t","resume_paused_queue":false,"resume_paused_queue":false}"#,
        ),
        (
            "a repeated op id",
            br#"{"op_id":"w4-op-p","op_id":"w4-op-p","mode":"queue","text":"t"}"#,
        ),
        (
            "a key that differs only in case",
            br#"{"op_id":"w4-op-p","Mode":"queue","text":"t"}"#,
        ),
        ("a byte order mark", b"\xef\xbb\xbf{\"op_id\":\"w4-op-p\",\"mode\":\"queue\",\"text\":\"t\"}"),
    ];
    for (what, payload) in cases {
        let r = post(&s, "thr_syn_0001", &good_headers(), payload);
        refused_body(&fake, what, &r, 400, "invalid_request");
        assert!(
            !fake.record_path("w4-op-p").exists(),
            "{what}: a record was claimed"
        );
    }
    assert!(
        !fake.state.join("sends").exists(),
        "the send record was opened"
    );
    // The allowed shape, keys in any order, both flags present or absent.
    for (n, payload) in [
        r#"{"text":"Synthetic W4 order","mode":"queue","op_id":"w4-op-q1"}"#,
        r#"{"op_id":"w4-op-q2","mode":"steer","text":"Synthetic W4 order","resume_paused_queue":false,"accept_possible_duplicate":false}"#,
    ]
    .iter()
    .enumerate()
    {
        let r = post(&s, "thr_syn_0001", &good_headers(), payload.as_bytes());
        assert_eq!(r.status, 200, "{n}: {}", r.text());
    }
    assert_eq!(fake.tell_count(), 2);
}
