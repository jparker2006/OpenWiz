//! U6 and U12, the server's half: creates are never coalesced,
//! and a repeat never creates twice.
//!
//! - **U6:** two creates with different op ids are two calls, even when
//! identical; the same op id from two tabs at once is one call; a
//! conflicting reuse is none. The read routes' single-flight is not on
//! the create path.
//! - **U12:** double presses, repeats, client retries and reloads all send
//! the same op id, and make one provider call: the core replays.
//!
//! The page's half (one op id per draft attempt, stored first) is U12's
//! browser proof (`apps/web/test/browser/u12-no-double.test.mjs`).

use std::thread;

use serde_json::{Value, json};

use crate::http::{Response, TestServer};
use crate::new_support::{NewFake, PROJECT, TEXT, body, create, file, serve_new, threads};
use crate::send_support::{good_headers, raw};

fn slow_spawn(test: &str) -> NewFake {
    NewFake::with(
        test,
        vec![json!({
            "any_spawn": true,
            "delay_ms": 800,
            "stdout_file": file("spawn-receipt-created.json"),
            "creates": true,
        })],
    )
}

/// Creates `values` at once, and returns the answers in order.
fn at_once(s: &TestServer, values: &[Value]) -> Vec<Response> {
    let handles: Vec<_> = values
        .iter()
        .map(|value| {
            let (addr, payload) = (s.addr, value.to_string());
            thread::spawn(move || {
                crate::http::send(
                    addr,
                    &raw(
                        "POST",
                        &threads(PROJECT),
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

fn status_word(r: &Response) -> String {
    assert_eq!(r.status, 200, "{}", r.text());
    r.json()["status"].as_str().expect("status").to_owned()
}

#[test]
fn u6_identical_creates_with_different_op_ids_are_two_calls() {
    let fake = NewFake::new("web_u6_sequential");
    let s = serve_new(&fake);
    for op in ["u6-op-1", "u6-op-2"] {
        let r = create(&s, PROJECT, &body(op, TEXT));
        assert_eq!(status_word(&r), "created", "{op}");
        assert_eq!(r.json()["result"]["replayed"], false, "{op}");
    }
    assert_eq!(fake.spawn_count(), 2, "never coalesced");
}

#[test]
fn u6_identical_creates_at_once_are_never_shared() {
    // One create at a time: the second waits for the slot, then runs
    // its own call. Nothing is shared between them.
    let fake = slow_spawn("web_u6_at_once");
    let s = serve_new(&fake);
    let answers = at_once(&s, &[body("u6-op-3", TEXT), body("u6-op-4", TEXT)]);
    for r in &answers {
        assert_eq!(status_word(r), "created");
        assert_eq!(r.json()["result"]["replayed"], false);
    }
    assert_ne!(answers[0].json()["op_id"], answers[1].json()["op_id"]);
    assert_eq!(fake.spawn_count(), 2);
}

#[test]
fn u6_u12_the_same_op_id_from_two_tabs_at_once_is_one_call() {
    let fake = slow_spawn("web_u6_two_tabs");
    let s = serve_new(&fake);
    let answers = at_once(&s, &[body("u6-op-5", TEXT), body("u6-op-5", TEXT)]);
    let words: Vec<String> = answers.iter().map(status_word).collect();
    assert!(words.contains(&"created".to_owned()), "{words:?}");
    for (r, word) in answers.iter().zip(&words) {
        // The other tab's answer is the replay, or "in progress" if it
        // arrived while the first was running: never a second create.
        let kind = r.json()["result"]["kind"].as_str().unwrap().to_owned();
        assert!(
            (word == "created" && kind == "outcome") || kind == "in_progress",
            "{word} {kind}: {}",
            r.text()
        );
    }
    assert_eq!(fake.spawn_count(), 1);
    assert_eq!(fake.threads_created(), 1);
}

#[test]
fn u6_a_conflicting_reuse_is_refused_with_no_call() {
    let fake = NewFake::new("web_u6_conflict");
    let s = serve_new(&fake);
    let r = create(&s, PROJECT, &body("u6-op-6", TEXT));
    assert_eq!(status_word(&r), "created");
    for (field, value) in [
        ("text", json!("Synthetic A2 changed prompt")),
        ("model", json!("syn-codex-model-02")),
        ("effort", json!("low")),
        ("harness", json!("claude-code")),
    ] {
        let mut other = body("u6-op-6", TEXT);
        other[field] = value;
        let r = create(&s, PROJECT, &other);
        assert_eq!(r.status, 200, "{field}: {}", r.text());
        assert_eq!(r.json()["result"]["kind"], "conflicting_reuse", "{field}");
        assert_eq!(r.json()["result"]["differs"], json!([field]), "{field}");
    }
    // Another project with the same op id, too.
    let r = create(&s, "prj_syn_0006", &body("u6-op-6", TEXT));
    assert_eq!(r.json()["result"]["kind"], "conflicting_reuse");
    assert_eq!(fake.spawn_count(), 1, "no call for any reuse");
}

#[test]
fn u12_repeats_and_client_retries_are_one_call() {
    let fake = NewFake::new("web_u12_repeats");
    let s = serve_new(&fake);
    let mut answers = vec![];
    for _ in 0..4 {
        answers.push(create(&s, PROJECT, &body("u12-op-1", TEXT)));
    }
    assert_eq!(status_word(&answers[0]), "created");
    assert_eq!(answers[0].json()["result"]["replayed"], false);
    for r in &answers[1..] {
        assert_eq!(status_word(r), "created");
        assert_eq!(r.json()["result"]["replayed"], true, "{}", r.text());
        assert_eq!(
            r.json()["result"]["outcome"],
            answers[0].json()["result"]["outcome"]
        );
    }
    // Five at once, the same op id: still the one call.
    let more = at_once(&s, &vec![body("u12-op-1", TEXT); 5]);
    for r in &more {
        assert_eq!(status_word(r), "created");
    }
    assert_eq!(fake.spawn_count(), 1);
    assert_eq!(fake.threads_created(), 1);
}
