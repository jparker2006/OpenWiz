//! W6: sends are never coalesced. Two sends with different op ids are
//! two calls, even when identical (U7's in-flight guard refuses the second
//! while the first is running, unless the owner accepts a possible
//! duplicate); the same op id from two tabs is one call; a conflicting
//! reuse is none. The read routes' single-flight is not on the send path.

use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use crate::http::Response;
use crate::msg::{self, SendFake};
use crate::send_support::{body, good_headers, messages, raw, send_json, serve_send};

fn slow_tell(test: &str) -> SendFake {
    SendFake::with(
        test,
        vec![msg::tell_response(
            "thr_syn_0001",
            "queue",
            json!({
                "delay_ms": 800,
                "stdout_file": msg::send_fixtures().join("tell-sent.json").to_str().unwrap(),
            }),
        )],
    )
}

/// Sends `values` at once, and returns the answers in order.
fn at_once(s: &crate::http::TestServer, values: &[Value]) -> Vec<Response> {
    let handles: Vec<_> = values
        .iter()
        .map(|value| {
            let (addr, payload) = (s.addr, value.to_string());
            thread::spawn(move || {
                crate::http::send(
                    addr,
                    &raw(
                        "POST",
                        &messages("thr_syn_0001"),
                        &good_headers(),
                        payload.as_bytes(),
                        true,
                    ),
                )
            })
        })
        .collect();
    handles.into_iter().map(|h| h.join().unwrap()).collect()
}

fn status(r: &Response) -> String {
    assert_eq!(r.status, 200, "{}", r.text());
    r.json()["status"].as_str().expect("status").to_owned()
}

#[test]
fn identical_sends_with_different_op_ids_are_two_calls() {
    let fake = SendFake::new("web_w6_sequential", "send");
    let s = serve_send(&fake);
    for op in ["w6-op-1", "w6-op-2"] {
        let r = send_json(&s, "thr_syn_0001", &body(op, "queue", "Synthetic W6 text"));
        assert_eq!(status(&r), "accepted");
    }
    assert_eq!(fake.tell_count(), 2);
}

#[test]
fn identical_sends_at_once_are_two_calls_only_when_the_owner_accepts() {
    // Without the acknowledgement, the second is refused while the first runs.
    let fake = slow_tell("web_w6_at_once");
    let s = serve_send(&fake);
    let mut second = body("w6-op-4", "queue", "Synthetic W6 text");
    let first = body("w6-op-3", "queue", "Synthetic W6 text");
    let h = {
        let (addr, payload) = (s.addr, first.to_string());
        thread::spawn(move || {
            crate::http::send(
                addr,
                &raw(
                    "POST",
                    &messages("thr_syn_0001"),
                    &good_headers(),
                    payload.as_bytes(),
                    true,
                ),
            )
        })
    };
    wait_for_tell(&fake, 1);
    let r = send_json(&s, "thr_syn_0001", &second);
    assert_eq!(status(&r), "possible_duplicate_refused");
    assert_eq!(status(&h.join().unwrap()), "accepted");
    assert_eq!(fake.tell_count(), 1);
    // With it, both are sent: never merged into one.
    let fake = slow_tell("web_w6_at_once_accepted");
    let s = serve_send(&fake);
    second["accept_possible_duplicate"] = json!(true);
    let mut first = first;
    first["accept_possible_duplicate"] = json!(true);
    let answers = at_once(&s, &[first, second]);
    for r in &answers {
        assert_eq!(status(r), "accepted");
    }
    assert_eq!(fake.tell_count(), 2);
}

#[test]
fn the_same_op_id_from_two_tabs_is_one_call() {
    let fake = slow_tell("web_w6_two_tabs");
    let s = serve_send(&fake);
    let value = body("w6-op-5", "queue", "Synthetic W6 tabs");
    let answers = at_once(&s, &[value.clone(), value.clone(), value.clone()]);
    let statuses: Vec<String> = answers.iter().map(status).collect();
    assert!(statuses.iter().any(|s| s == "accepted"), "{statuses:?}");
    for st in &statuses {
        assert!(st == "accepted" || st == "in_progress", "{statuses:?}");
    }
    assert_eq!(fake.tell_count(), 1);
    // A later retry replays the recorded outcome, with no call.
    let r = send_json(&s, "thr_syn_0001", &value);
    assert_eq!(status(&r), "accepted");
    assert_eq!(r.json()["result"]["replayed"], json!(true));
    assert_eq!(fake.tell_count(), 1);
}

#[test]
fn a_conflicting_reuse_is_no_call() {
    let fake = SendFake::new("web_w6_conflict", "send");
    let s = serve_send(&fake);
    let value = body("w6-op-6", "queue", "Synthetic W6 conflict");
    assert_eq!(status(&send_json(&s, "thr_syn_0001", &value)), "accepted");
    let mut text = value.clone();
    text["text"] = json!("Synthetic W6 conflict, changed");
    let mut mode = value.clone();
    mode["mode"] = json!("steer");
    for (id, changed) in [
        ("thr_syn_0001", text),
        ("thr_syn_0001", mode),
        ("thr_syn_0003", value),
    ] {
        let r = send_json(&s, id, &changed);
        assert_eq!(status(&r), "conflicting_reuse", "{id} {changed}");
    }
    assert_eq!(fake.tell_count(), 1);
}

#[test]
fn the_send_path_has_no_single_flight() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../crates/server/src/send.rs");
    let text = std::fs::read_to_string(path).expect("crates/server/src/send.rs");
    for word in ["Runs", "Call::", "flights", "single-flight"] {
        assert!(!text.contains(word), "the send path mentions {word}");
    }
}

fn wait_for_tell(fake: &SendFake, n: u64) {
    let until = std::time::Instant::now() + Duration::from_secs(10);
    while fake.tell_count() < n {
        assert!(std::time::Instant::now() < until, "no tell {n}");
        thread::sleep(Duration::from_millis(10));
    }
}
