//! N1 end to end (A1 phase 2): `openwiz new --yes` makes exactly one
//! `thread spawn`, with `--project`, `--machine cloud`, `--provider`,
//! `--model`, `--reasoning-level`, `--request-id`, `--json` and the text as
//! one `--prompt` argument, byte for byte (a leading `-`, line breaks,
//! Unicode, the cap). Never an excluded flag, never an omitted setting. The
//! release build runs it against the fake; the fake logs what it got.

use openwiz_core::messaging::MAX_TEXT_BYTES;

use crate::fake::*;

pub const NEVER: &[&str] = &[
    "--file",
    "--image",
    "--environment",
    "--new-environment",
    "--base-branch",
    "--host",
    "--new-machine",
    "--machine-inputs",
    "--parent-thread",
    "--parent-self",
    "--title",
    "--service-tier",
    "--permission-mode",
    "--plan",
    "--section",
    "--visibility",
    "--environment-provider",
    "--environment-inputs",
    "--send-at",
    "--origin-kind",
    "--source-thread",
    "--source-seq-end",
    "--prompt-file",
];

/// The flags every spawn passes, each once, each with a value.
const ALWAYS: &[&str] = &[
    "--project",
    "--machine",
    "--provider",
    "--model",
    "--reasoning-level",
    "--request-id",
    "--prompt",
];

/// Checks one spawn's shape: every setting explicit, nothing else.
pub fn assert_spawn_shape(argv: &[String]) {
    assert_eq!(argv.len(), 17, "{argv:?}");
    assert_eq!(&argv[..2], ["thread", "spawn"]);
    for flag in ALWAYS {
        assert_eq!(
            argv[..16].iter().filter(|a| a == flag).count(),
            1,
            "{flag} once: {argv:?}"
        );
    }
    for flag in NEVER {
        assert!(!argv[..16].iter().any(|a| a == flag), "{flag}: {argv:?}");
    }
    assert_eq!(argv[5], "cloud", "--machine cloud");
    assert_eq!(argv[14], "--json");
    assert_eq!(argv[15], "--prompt", "the text is the last argument");
}

#[test]
fn one_create_makes_one_spawn_in_exactly_the_shape() {
    let fake = NewFake::new("n01_one");
    let out = fake.run_process(
        &new_args(
            "Synthetic project one",
            "op-n01-one",
            &["--json"],
            "Synthetic hello",
        ),
        "",
    );
    let (code, json) = json_of(&out);
    assert_eq!(code, Some(0), "{json}");
    assert_eq!(json["status"], "created", "{json}");
    assert_eq!(fake.spawn_count(), 1);
    let spawns = fake.spawns();
    assert_eq!(
        spawns,
        vec![spawn_argv(PROJECT, "op-n01-one", "Synthetic hello")]
    );
    assert_spawn_shape(&spawns[0]);
    // The reads come first, in the same run; the start read after the call.
    let words: Vec<String> = fake
        .invocations()
        .iter()
        .map(|a| a[..2.min(a.len())].join(" "))
        .collect();
    assert_eq!(
        words,
        [
            "--version",
            "project list",
            "cloud status",
            "thread spawn",
            "thread show"
        ],
        "{words:?}"
    );
}

#[test]
fn the_text_is_one_argument_byte_for_byte() {
    let fake = NewFake::new("n01_bytes");
    let cap = "a".repeat(MAX_TEXT_BYTES);
    let texts = [
        "-starts with a dash",
        "--json",
        "--prompt",
        "--file /synthetic/notes.txt",
        "--reasoning-level=max",
        "-- after the end of options",
        "line one\nline two\n",
        "\ttabbed\r\nand CRLF",
        "Unicode: é ü 漢字 😀 \u{200b}",
        "   leading spaces",
        cap.as_str(),
    ];
    for (i, text) in texts.iter().enumerate() {
        let op = format!("op-n01-bytes-{i}");
        let (code, out, err) = fake.cli(&new_args("Synthetic project one", &op, &[], text), "");
        assert_eq!(code, 0, "{text:?}\n{out}\n{err}");
        let spawns = fake.spawns();
        let last = spawns.last().expect("a spawn");
        assert_eq!(last, &spawn_argv(PROJECT, &op, text), "{text:?}");
        assert_spawn_shape(last);
    }
    assert_eq!(fake.spawn_count(), texts.len() as u64);
}

#[test]
fn the_text_from_stdin_reaches_the_spawn_unchanged() {
    let fake = NewFake::new("n01_stdin");
    let text = "-Synthetic from stdin\nwith a second line\n";
    let mut args = new_args("Synthetic project one", "op-n01-stdin", &["--json"], "");
    args.truncate(args.len() - 2);
    let out = fake.run_process(&args, text);
    let (code, json) = json_of(&out);
    assert_eq!(code, Some(0), "{json}");
    assert_eq!(
        fake.spawns(),
        vec![spawn_argv(PROJECT, "op-n01-stdin", text)]
    );
}

#[test]
fn every_setting_is_passed_as_chosen_and_none_is_omitted() {
    let fake = NewFake::new("n01_settings");
    for (op, harness, model, effort) in [
        ("op-n01-claude", "claude-code", "syn-claude-model-01", "max"),
        ("op-n01-codex-low", "codex", "syn-codex-model-02", "low"),
    ] {
        let args = [
            "new",
            "--project-id",
            PROJECT,
            "--harness",
            harness,
            "--model",
            model,
            "--effort",
            effort,
            "--op-id",
            op,
            "--yes",
            "--",
            "Synthetic settings",
        ];
        let (code, out, err) = fake.cli(&args, "");
        assert_eq!(code, 0, "{out}\n{err}");
        let spawn = fake.spawns().pop().expect("a spawn");
        assert_spawn_shape(&spawn);
        assert_eq!(spawn[7], harness, "the provider is the mapped harness");
        assert_eq!(spawn[9], model);
        assert_eq!(spawn[11], effort);
    }
    // A setting left out is a usage error: nothing is read or called.
    let fake = NewFake::new("n01_omitted");
    for drop in ["--harness", "--model", "--effort"] {
        let mut args = new_args("Synthetic project one", "op-n01-omit", &[], "Synthetic");
        let i = args.iter().position(|a| *a == drop).unwrap();
        args.drain(i..i + 2);
        let (code, out, err) = fake.cli(&args, "");
        assert_eq!(code, 2, "{drop}: {out}");
        assert!(err.contains("nothing is defaulted"), "{err}");
    }
    assert!(fake.invocations().is_empty(), "a usage error reads nothing");
}
