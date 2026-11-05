//! W5 : the same operation. For every case of the `send` fixtures, the
//! route's answer is byte for byte what `openwiz send … --json` prints for
//! the same input, op id and clock, and the preview and lookup routes'
//! answers are `--preview --json` and `--check --json`. A static check
//! finds no send logic in the server: it calls the core's `send`,
//! `preview` and `lookup`, and works out no outcome, effect or label.

use serde_json::{Value, json};

use crate::msg::{self, SendFake};
use crate::send_support::{body, cli, cli_args, get_public, send_json, serve_send};

/// Every target of the `send` scenario, with what the fixture gives it.
const TARGETS: &[&str] = &[
    "thr_syn_0001",
    "thr_syn_0002",
    "thr_syn_0003",
    "thr_syn_0004",
    "thr_syn_0005",
    "thr_syn_0006",
    "thr_syn_0011",
    "thr_syn_0012",
    "thr_syn_0014",
    "thr_syn_0015",
    "thr_syn_9999",
];

/// The send through the route, and through the CLI on a twin fake: the
/// same status mapping and the same bytes.
fn same(web: &SendFake, cli_fake: &SendFake, id: &str, value: &Value) {
    let s = serve_send(web);
    let r = send_json(&s, id, value);
    let (code, out, err) = cli(cli_fake, &cli_args(id, value));
    match code {
        0 | 3 | 5..=9 => {
            assert_eq!(r.status, 200, "{id} {value}: exit {code}: {}", r.text());
            assert_eq!(r.text(), out, "{id} {value}: the bodies differ");
        }
        2 => {
            assert_eq!(r.status, 400, "{id} {value}: {}", r.text());
            let (kind, message) = r.error();
            assert_eq!(kind, "invalid_request");
            assert_eq!(
                err,
                format!("openwiz: {message}; nothing was sent (see openwiz --help)\n"),
                "{id} {value}"
            );
        }
        other => panic!("{id} {value}: unexpected CLI exit {other}: {err}"),
    }
    assert_eq!(
        web.tell_count(),
        cli_fake.tell_count(),
        "{id} {value}: tell counts"
    );
}

#[test]
fn every_send_fixture_case_equals_the_cli() {
    for (n, id) in TARGETS.iter().enumerate() {
        for mode in ["queue", "steer"] {
            for resume in [false, true] {
                let name = format!("web_w5_{n}_{mode}_{resume}");
                let web = SendFake::new(&format!("{name}_web"), "send");
                let cli_fake = SendFake::new(&format!("{name}_cli"), "send");
                let mut value = body("w5-op-1", mode, "Synthetic W5 text");
                if resume {
                    value["resume_paused_queue"] = json!(true);
                }
                same(&web, &cli_fake, id, &value);
                // A replay of the same op id, and a conflicting reuse.
                same(&web, &cli_fake, id, &value);
                let mut other = value.clone();
                other["text"] = json!("Synthetic W5 other text");
                same(&web, &cli_fake, id, &other);
            }
        }
    }
}

#[test]
fn an_uncertain_send_and_a_possible_duplicate_equal_the_cli() {
    let lost = || {
        vec![msg::tell_response(
            "thr_syn_0001",
            "queue",
            json!({ "stdout": "not json" }),
        )]
    };
    let web = SendFake::with("web_w5_uncertain_web", lost());
    let cli_fake = SendFake::with("web_w5_uncertain_cli", lost());
    let first = body("w5-op-u1", "queue", "Synthetic W5 lost text");
    same(&web, &cli_fake, "thr_syn_0001", &first);
    let mut again = body("w5-op-u2", "queue", "Synthetic W5 lost text");
    same(&web, &cli_fake, "thr_syn_0001", &again);
    again["accept_possible_duplicate"] = json!(true);
    same(&web, &cli_fake, "thr_syn_0001", &again);
}

#[test]
fn the_core_refusals_equal_the_cli() {
    let web = SendFake::new("web_w5_invalid_web", "send");
    let cli_fake = SendFake::new("web_w5_invalid_cli", "send");
    for (id, value) in [
        ("thr_syn_0001", body("w5-op-i", "queue", " ")),
        ("thr_syn_0001", body("w5-op-i", "queue", "")),
        ("thr_syn_0001", body("-bad", "queue", "Synthetic W5 text")),
        ("bad-", body("w5-op-i", "queue", "Synthetic W5 text")),
    ] {
        same(&web, &cli_fake, id, &value);
    }
}

#[test]
fn every_preview_equals_the_cli() {
    let web = SendFake::new("web_w5_preview_web", "send");
    let cli_fake = SendFake::new("web_w5_preview_cli", "send");
    let s = serve_send(&web);
    for id in TARGETS.iter().copied().chain(["bad-"]) {
        for mode in ["queue", "steer"] {
            let mut args = vec!["--preview", "--json"];
            let query = if mode == "steer" {
                args.push("--steer");
                "?mode=steer"
            } else {
                ""
            };
            args.extend(["--", id]);
            let (code, out, err) = cli_fake.cli(&args, "");
            let r = get_public(&s, &format!("/api/v1/sessions/{id}/preview{query}"));
            match code {
                0 | 3 | 5 => {
                    assert_eq!(r.status, 200, "{id} {mode}: {}", r.text());
                    assert_eq!(r.text(), out, "{id} {mode}");
                }
                2 => {
                    assert_eq!(r.status, 400, "{id} {mode}");
                    let message = r.error().1;
                    assert_eq!(
                        err,
                        format!("openwiz: {message}; nothing was sent (see openwiz --help)\n")
                    );
                }
                other => panic!("{id} {mode}: exit {other}: {err}"),
            }
        }
        // `mode=queue` is the default, spelled out.
        let a = get_public(&s, &format!("/api/v1/sessions/{id}/preview"));
        let b = get_public(&s, &format!("/api/v1/sessions/{id}/preview?mode=queue"));
        assert_eq!(a.body, b.body, "{id}");
    }
    for query in [
        "?mode=auto",
        "?mode=",
        "?mode=queue&mode=queue",
        "?steer=1",
        "?x",
    ] {
        let r = get_public(&s, &format!("/api/v1/sessions/thr_syn_0001/preview{query}"));
        assert_eq!(r.status, 400, "{query}: {}", r.text());
        assert_eq!(r.error().0, "invalid_request");
    }
    assert_eq!(web.tell_count(), 0);
    assert_eq!(cli_fake.tell_count(), 0);
}

#[test]
fn every_lookup_equals_the_cli() {
    let web = SendFake::new("web_w5_lookup_web", "send");
    let cli_fake = SendFake::new("web_w5_lookup_cli", "send");
    let s = serve_send(&web);
    let lookups = |s: &crate::http::TestServer| {
        for (id, op) in [
            ("thr_syn_0001", "w5-op-l1"),
            ("thr_syn_0002", "w5-op-l1"),
            ("thr_syn_0002", "w5-op-l2"),
            ("thr_syn_9999", "w5-op-l3"),
            ("thr_syn_0001", "w5-op-none"),
            ("thr_syn_0001", "-bad"),
            ("bad-", "w5-op-l1"),
        ] {
            let (code, out, err) =
                cli_fake.cli(&["--op-id", op, "--check", "--json", "--", id], "");
            let r = get_public(s, &format!("/api/v1/sessions/{id}/messages/{op}"));
            match code {
                0 | 3 | 5..=9 | 11 => {
                    assert_eq!(r.status, 200, "{id} {op}: exit {code}: {}", r.text());
                    assert_eq!(r.text(), out, "{id} {op}");
                }
                2 => {
                    assert_eq!(r.status, 400, "{id} {op}: {}", r.text());
                    let message = r.error().1;
                    assert_eq!(
                        err,
                        format!("openwiz: {message}; nothing was sent (see openwiz --help)\n")
                    );
                }
                other => panic!("{id} {op}: exit {other}: {err}"),
            }
        }
    };
    // Before any record exists: no record (and the record is not created).
    lookups(&s);
    assert!(
        !web.state.join("sends").exists(),
        "a lookup created the record"
    );
    // After sends through each side.
    for (id, op) in [
        ("thr_syn_0001", "w5-op-l1"),
        ("thr_syn_0002", "w5-op-l2"),
        ("thr_syn_9999", "w5-op-l3"),
    ] {
        let value = body(op, "queue", "Synthetic W5 lookup text");
        assert_eq!(send_json(&s, id, &value).status, 200);
        let _ = cli(&cli_fake, &cli_args(id, &value));
    }
    lookups(&s);
    assert_eq!(web.tell_count(), 2);
    let r = get_public(&s, "/api/v1/sessions/thr_syn_0001/messages/w5-op-l1?x=1");
    assert_eq!(r.status, 400);
}

#[test]
fn the_server_has_no_send_logic() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../crates/server/src");
    let mut calls = String::new();
    for entry in std::fs::read_dir(dir).expect("server sources") {
        let path = entry.expect("entry").path();
        let text = std::fs::read_to_string(&path).expect("source");
        calls.push_str(&text);

        if path.ends_with("attachments.rs") {
            assert!(
                !text.contains("messaging::"),
                "{path:?} reaches into messaging"
            );
            continue;
        }
        for word in [
            "Delivery",
            "FailReason",
            "UncertainReason",
            "WaitingOn",
            "Effects",
            "work_out",
            "reconcile",
            "Reconciliation",
            "PreviewStatus",
            "LookupResult",
            ".requested(",
            "SendResult",
            "exit_code",
        ] {
            assert!(
                !text.contains(word),
                "{path:?} works out send logic: {word}"
            );
        }
    }

    for call in ["send::send_with(", "preview::preview(", "lookup::lookup("] {
        assert!(
            calls.contains(call),
            "the server does not call the core's {call}"
        );
    }
}
