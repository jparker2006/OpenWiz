//! AW4 : the same operations. For every B1 and B2 fixture case,
//! each attachment route's answer is what `openwiz attachment add`, `ls` or
//! `rm --json` prints for the same bytes, name, store and clock, and a send
//! or a create with attachments answers what `openwiz send` or `openwiz new`
//! prints with `--attachment`. Only the random attachment ids, and an
//! upload's `path` (the name: an upload has no path), differ, and are made
//! equal before comparing. A static check finds no kind, name, limit,
//! floor, gate or retention rule in the server or the web.

use std::fs;
use std::path::Path;

use openwiz_core::model::Timestamp;
use openwiz_server::Limits;
use serde_json::{Value, json};

use crate::attach_support::{
    Free, HTML_WITH_SCRIPT, SVG_WITH_SCRIPT, State, cli, gif, heic, ids_in, jpeg, list, normalized,
    ready_id, remove, serve_attach, upload, webp, zip,
};
use crate::new_support::clock;
use crate::staging::{png, text};

const MIB: usize = 1024 * 1024;

/// Every B1 kind and refusal, as `(name, bytes)`: B1's AP1, AP2, AP3 and
/// the check, the check cases, made here (synthetic).
fn cases() -> Vec<(String, Vec<u8>)> {
    vec![
        ("aw4.png".into(), png(2000)),
        ("aw4.jpg".into(), jpeg(2000)),
        ("aw4.jpeg".into(), jpeg(3000)),
        ("aw4.gif".into(), gif(2000)),
        ("aw4.webp".into(), webp(2000)),
        ("aw4.txt".into(), text("Synthetic AW4 text\n")),
        ("aw4.md".into(), text("# Synthetic AW4\n")),
        ("aw4.zip".into(), zip(2000)),
        ("aw4.svg".into(), SVG_WITH_SCRIPT.as_bytes().to_vec()),
        ("aw4.html".into(), HTML_WITH_SCRIPT.as_bytes().to_vec()),
        ("aw4-empty.txt".into(), vec![]),
        ("no-extension".into(), text("Synthetic AW4\n")),
        // HEIC is refused, never converted.
        ("aw4.heic".into(), heic(2000)),
        // The name and the bytes disagree (the check).
        ("png-as.jpg".into(), png(2000)),
        ("text-as.png".into(), text("Synthetic AW4 not an image\n")),
        ("svg-as.png".into(), SVG_WITH_SCRIPT.as_bytes().to_vec()),
        ("png-as.svg".into(), png(2000)),
        // A polyglot: a PNG signature with markup after it.
        ("polyglot.png".into(), {
            let mut b = png(64);
            b.extend_from_slice(HTML_WITH_SCRIPT.as_bytes());
            b
        }),
        // The provider limits.
        ("at-limit.png".into(), png(10 * MIB)),
        ("over-limit.png".into(), png(10 * MIB + 1)),
        ("image-typed.bmp".into(), text(&"b".repeat(10 * MIB + 1))),
        // Names the core makes safe.
        ("Synthétique résumé (1).txt".into(), text("Synthetic AW4\n")),
        (".hidden.txt".into(), text("Synthetic AW4\n")),
        ("-dash.txt".into(), text("Synthetic AW4\n")),
        (format!("{}.txt", "n".repeat(240)), text("Synthetic AW4\n")),
    ]
}

/// `openwiz attachment add <dir>/<name> --json` on `state`.
fn add_by_cli(state: &Path, sources: &Path, name: &str, bytes: &[u8]) -> (i32, String) {
    crate::dirs::private_dir_all(sources).unwrap();
    let path = sources.join(name);
    fs::write(&path, bytes).unwrap();
    let path = path.to_str().unwrap().to_owned();
    let (code, out, err) = cli(
        state,
        &clock(),
        &Free(std::sync::atomic::AtomicU64::new(1 << 50)),
        &["add", "--json", "--", &path],
    );
    assert!(err.is_empty(), "{name}: {err}");
    (
        code,
        out.replace(
            &format!("\"{}\"", json_escape(&path)),
            &format!("\"{}\"", json_escape(name)),
        ),
    )
}

fn json_escape(s: &str) -> String {
    let quoted = serde_json::to_string(s).unwrap();
    quoted[1..quoted.len() - 1].to_owned()
}

#[test]
fn every_upload_case_equals_attachment_add() {
    let web = State::new("web_aw4_add_web");
    let by_cli = State::new("web_aw4_add_cli");
    let sources = by_cli.dir.with_file_name("web_aw4_add_cli-sources");
    let _ = fs::remove_dir_all(&sources);
    let s = serve_attach(&web, Free::plenty(), Limits::default());
    for (name, bytes) in cases() {
        let r = upload(&s, &name, &bytes);
        let (code, out) = add_by_cli(&by_cli.dir, &sources, &name, &bytes);
        assert!(code == 0 || code == 5, "{name}: exit {code}: {out}");
        assert_eq!(r.status, 200, "{name}: {}", r.text());
        let route = r.json();
        let ids_web = ids_in(&route);
        let ids_cli = ids_in(&serde_json::from_str(&out).unwrap());
        assert_eq!(ids_web.len(), ids_cli.len(), "{name}");
        let pairs: Vec<(&str, &str)> = ids_web
            .iter()
            .zip(&ids_cli)
            .map(|(w, c)| (w.as_str(), c.as_str()))
            .collect();
        assert_eq!(
            normalized(&r.text(), &pairs),
            out,
            "{name}: the route's answer differs from openwiz attachment add"
        );
        assert_eq!(
            code == 0,
            route["ready"].as_array().unwrap().len() == 1,
            "{name}"
        );
    }
    let _ = fs::remove_dir_all(&sources);
}

#[test]
fn the_list_equals_attachment_ls() {
    let state = State::new("web_aw4_ls");
    let s = serve_attach(&state, Free::plenty(), Limits::default());
    // Never staged: the empty list, and nothing is made.
    let r = list(&s);
    let (code, out, _) = cli(
        &state.dir,
        &clock(),
        &Free(std::sync::atomic::AtomicU64::new(1 << 50)),
        &["ls", "--json"],
    );
    assert_eq!((r.status, code), (200, 0));
    assert_eq!(r.text(), out);
    assert!(
        !state.dir.join("attachments").exists(),
        "a list made the store"
    );
    for (name, bytes) in [("aw4-a.png", png(500)), ("aw4-b.txt", text("Synthetic\n"))] {
        ready_id(&upload(&s, name, &bytes));
    }
    let r = list(&s);
    let (code, out, _) = cli(
        &state.dir,
        &clock(),
        &Free(std::sync::atomic::AtomicU64::new(1 << 50)),
        &["ls", "--json"],
    );
    assert_eq!((r.status, code), (200, 0));
    assert_eq!(r.text(), out, "the list differs from openwiz attachment ls");
    assert_eq!(r.json()["attachments"].as_array().unwrap().len(), 2);
}

#[test]
fn an_expired_attachment_is_listed_and_swept_as_attachment_ls_does() {
    let web = State::new("web_aw4_expired_web");
    let by_cli = State::new("web_aw4_expired_cli");
    let before = Timestamp(clock().0.0 - 25 * 60 * 60 * 1000);
    for state in [&web, &by_cli] {
        crate::staging::stage(&state.dir, "aw4-old.txt", b"Synthetic old\n", before);
    }
    let s = serve_attach(&web, Free::plenty(), Limits::default());
    let r = list(&s);
    let (code, out, _) = cli(
        &by_cli.dir,
        &clock(),
        &Free(std::sync::atomic::AtomicU64::new(1 << 50)),
        &["ls", "--json"],
    );
    assert_eq!((r.status, code), (200, 0));
    let route = r.json();
    let pairs: Vec<(String, String)> = ids_in(&route)
        .into_iter()
        .zip(ids_in(&serde_json::from_str(&out).unwrap()))
        .collect();
    let pairs: Vec<(&str, &str)> = pairs
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    assert_eq!(normalized(&r.text(), &pairs), out);
    assert_eq!(route["expired"].as_array().unwrap().len(), 1, "{route}");
    assert!(web.nothing_staged(), "the expired copy was deleted");
}

#[test]
fn every_removal_equals_attachment_rm() {
    let web = State::new("web_aw4_rm_web");
    let by_cli = State::new("web_aw4_rm_cli");
    let s = serve_attach(&web, Free::plenty(), Limits::default());
    let w = ready_id(&upload(&s, "aw4-rm.txt", b"Synthetic rm\n"));
    let c = crate::staging::id_of(&crate::staging::stage(
        &by_cli.dir,
        "aw4-rm.txt",
        b"Synthetic rm\n",
        clock().0,
    ));
    for _ in 0..2 {
        // Removed, then not found.
        let r = remove(&s, &w);
        let (code, out, _) = cli(
            &by_cli.dir,
            &clock(),
            &Free(std::sync::atomic::AtomicU64::new(1 << 50)),
            &["rm", "--json", "--", &c],
        );
        assert!(code == 0 || code == 3, "exit {code}: {out}");
        assert_eq!(r.status, 200, "{}", r.text());
        assert_eq!(normalized(&r.text(), &[(&w, &c)]), out);
    }
    // Not an attachment id: the CLI's own words.
    for bad in ["-bad", "a.b", &"x".repeat(65)] {
        let r = remove(&s, bad);
        let (code, _, err) = cli(
            &by_cli.dir,
            &clock(),
            &Free(std::sync::atomic::AtomicU64::new(1 << 50)),
            &["rm", "--json", "--", bad],
        );
        assert_eq!((r.status, code), (400, 2), "{bad}: {}", r.text());
        let (kind, message) = r.error();
        assert_eq!(kind, "invalid_request");
        assert_eq!(
            err,
            format!("openwiz: {message}; nothing was staged or removed (see openwiz --help)\n")
        );
    }
}

/// A queue tell answered with the `send` scenario's `file`, any attachments.
fn tell(target: &str, file: &str) -> Value {
    let mut response =
        crate::msg::tell_response(target, "queue", json!({ "any_attachments": true }));
    let path = crate::msg::send_fixtures().join(file);
    assert!(path.is_file(), "no fixture {path:?}");
    response["stdout_file"] = json!(path.to_str().unwrap());
    response
}

#[test]
fn a_send_with_attachments_equals_openwiz_send() {
    let target = "thr_syn_0001";
    let web = crate::msg::SendFake::with("web_aw4_send_web", vec![tell(target, "tell-sent.json")]);
    let by_cli =
        crate::msg::SendFake::with("web_aw4_send_cli", vec![tell(target, "tell-sent.json")]);
    let s = crate::aw05_gate::serve_send_attach(&web);
    let mut ids = vec![];
    for fake in [&web, &by_cli] {
        ids.push(
            [
                ("aw4-a.txt", text("Synthetic a\n")),
                ("aw4-b.png", png(400)),
            ]
            .iter()
            .map(|(n, b)| {
                crate::staging::id_of(&crate::staging::stage(&fake.state, n, b, clock().0))
            })
            .collect::<Vec<_>>(),
        );
    }
    let body = json!({
        "op_id": "aw4-op-1", "mode": "queue", "text": "Synthetic AW4 send",
        "attachments": ids[0],
    });
    let args = [
        "--op-id",
        "aw4-op-1",
        "--attachment",
        &ids[1][0],
        "--attachment",
        &ids[1][1],
        "--json",
        "--",
        target,
        "Synthetic AW4 send",
    ];
    let pairs: Vec<(&str, &str)> = ids[0]
        .iter()
        .zip(&ids[1])
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    for round in 0..2 {
        // The send, then its replay.
        let r = crate::send_support::send_json(&s, target, &body);
        let (code, out, err) = by_cli.cli(&args, "");
        assert_eq!(code, 0, "round {round}: {err}");
        assert_eq!(r.status, 200, "round {round}: {}", r.text());
        assert_eq!(normalized(&r.text(), &pairs), out, "round {round}");
    }
    assert_eq!(web.tell_count(), 1);
    assert_eq!(by_cli.tell_count(), 1);
    let (tw, tc) = (&web.tells()[0], &by_cli.tells()[0]);
    let flags = |t: &Vec<String>| {
        t.iter()
            .filter(|a| a.starts_with("--file") || a.starts_with("--image"))
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(flags(tw), flags(tc), "the same flags, files first");
}

#[test]
fn a_gate_refusal_equals_openwiz_sends() {
    let target = "thr_syn_0001";
    let web = crate::msg::SendFake::new("web_aw4_gate_web", "send");
    let by_cli = crate::msg::SendFake::new("web_aw4_gate_cli", "send");
    let s = crate::aw05_gate::serve_send_attach(&web);
    let missing = "att-00000000000000000000000000000000";
    let body = json!({
        "op_id": "aw4-op-2", "mode": "queue", "text": "Synthetic AW4 gate",
        "attachments": [missing],
    });
    let r = crate::send_support::send_json(&s, target, &body);
    let (code, _, err) = by_cli.cli(
        &[
            "--op-id",
            "aw4-op-2",
            "--attachment",
            missing,
            "--json",
            "--",
            target,
            "Synthetic AW4 gate",
        ],
        "",
    );
    assert_eq!((r.status, code), (400, 2), "{}", r.text());
    let (kind, message) = r.error();
    assert_eq!(kind, "invalid_request");
    assert_eq!(
        err,
        format!("openwiz: {message}; nothing was sent (see openwiz --help)\n")
    );
    assert_eq!(web.tell_count() + by_cli.tell_count(), 0);
}

#[test]
fn a_create_with_attachments_equals_openwiz_new() {
    use crate::new_support::{EFFORT, HARNESS, MODEL, NewFake, PROJECT};
    let web = NewFake::new("web_aw4_new_web");
    let by_cli = NewFake::new("web_aw4_new_cli");
    let s = crate::aw05_gate::serve_new_attach(&web);
    let mut ids = vec![];
    for fake in [&web, &by_cli] {
        ids.push(crate::staging::id_of(&crate::staging::stage(
            &fake.state,
            "aw4-new.png",
            &png(600),
            clock().0,
        )));
    }
    let text = "Synthetic AW4 prompt";
    let body = json!({
        "op_id": "aw4-op-3", "harness": HARNESS, "model": MODEL, "effort": EFFORT,
        "text": text, "attachments": [ids[0]],
    });
    let base = [
        "new",
        "--project-id",
        PROJECT,
        "--harness",
        HARNESS,
        "--model",
        MODEL,
        "--effort",
        EFFORT,
        "--op-id",
        "aw4-op-3",
        "--attachment",
        &ids[1],
        "--json",
    ];
    let pairs = [(ids[0].as_str(), ids[1].as_str())];
    // The preview first: it reads, and creates nothing.
    let r = crate::new_support::preview_json(&s, PROJECT, &body);
    let mut args: Vec<&str> = base.to_vec();
    args.extend(["--", text]);
    let (code, out, err) = crate::aw05_gate::new_cli(&by_cli, &args);
    assert_eq!(code, 12, "{err}");
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(normalized(&r.text(), &pairs), out, "the preview");
    let r = crate::new_support::create(&s, PROJECT, &body);
    let mut args: Vec<&str> = base.to_vec();
    args.extend(["--yes", "--", text]);
    let (code, out, err) = crate::aw05_gate::new_cli(&by_cli, &args);
    assert_eq!(code, 0, "{err}");
    assert_eq!(r.status, 200, "{}", r.text());
    // The web's answer also carries the start read once; the CLI's
    // follows it. Everything else is the same.
    let mut route = r.json();
    let mut printed: Value = serde_json::from_str(&out).unwrap();
    route.as_object_mut().unwrap().remove("start");
    printed.as_object_mut().unwrap().remove("start");
    assert_eq!(normalized(&route.to_string(), &pairs), printed.to_string());
    assert_eq!(web.spawn_count(), 1);
    assert_eq!(by_cli.spawn_count(), 1);
}

const SERVER_SRC: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../crates/server/src");
const WEB_SRC: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../apps/web/src");

fn sources(dir: &Path, out: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(dir).expect("sources") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            sources(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e == "rs" || e == "ts" || e == "tsx")
        {
            out.push((
                path.display().to_string(),
                fs::read_to_string(&path).unwrap(),
            ));
        }
    }
}

/// The core's attachment rules, by name and by value: none of them may be
/// worked out again outside the core and the store. The Rust names apply
/// to the server; the values and words to both.
const SERVER_RULES: &[&str] = &[
    "classify(",
    "limit(",
    "check(",
    "staging_room(",
    "retention(",
    "can_remove(",
    "claimable(",
    "gate(",
    "SafeName",
    "IMAGE_MAX_BYTES",
    "FREE_DISK_FLOOR_BYTES",
    "IMAGE_TYPED_EXTENSIONS",
    "SNIFF_BYTES",
    "UNSENT_TTL",
    "Kind::Image",
    "Kind::File",
    "FailReason::",
];

const VALUES: &[&str] = &[
    "10 * 1024 * 1024",
    "25 * 1024 * 1024",
    "1024 * 1024 * 1024",
    "10485760",
    "26214400",
    "26_214_400",
    "1073741824",
    "86400",
    "24 * 60 * 60",
    "heic",
    "heif",
    "ftyp",
    ".bmp",
    ".tiff",
];

#[test]
fn no_attachment_rule_is_worked_out_in_the_server_or_the_web() {
    let (mut server, mut web) = (vec![], vec![]);
    sources(Path::new(SERVER_SRC), &mut server);
    sources(Path::new(WEB_SRC), &mut web);
    assert!(
        server.len() >= 8 && web.len() >= 10,
        "the sources were read"
    );
    let mut found = vec![];
    for (path, text) in server.iter().chain(&web) {
        let rules = if path.ends_with(".rs") {
            SERVER_RULES
        } else {
            &[]
        };
        for rule in rules.iter().chain(VALUES) {
            if text.to_lowercase().contains(&rule.to_lowercase()) {
                found.push(format!("{path}: {rule}"));
            }
        }
        // The server's one use of a limit: its upload cap is the core's
        // per-file limit (the check), named once in lib.rs, never a number of its
        // own.
        let caps = text.matches("FILE_MAX_BYTES").count();
        let allowed = usize::from(path.ends_with("server/src/lib.rs"));
        if caps != allowed {
            found.push(format!("{path}: FILE_MAX_BYTES {caps} times"));
        }
    }
    assert!(
        found.is_empty(),
        "attachment rules outside the core: {found:#?}"
    );
}

#[test]
fn the_web_checks_bytes_only_for_a_safe_preview() {
    let mut all = vec![];
    sources(Path::new(WEB_SRC), &mut all);
    assert!(
        all.iter().any(|(p, _)| p.ends_with("src/attachments.ts")),
        "apps/web/src/attachments.ts holds the preview rule"
    );
    let signatures = ["0x89", "0xff, 0xd8", "GIF8", "WEBP", "RIFF"];
    for (path, text) in &all {
        let here = signatures.iter().filter(|s| text.contains(**s)).count();
        if path.ends_with("src/attachments.ts") {
            assert_eq!(here, signatures.len(), "{path}: the preview signatures");
            assert_eq!(
                text.matches("export function previewable(").count(),
                1,
                "{path}"
            );
        } else {
            assert_eq!(here, 0, "{path} checks bytes outside the preview rule");
        }
    }
}
