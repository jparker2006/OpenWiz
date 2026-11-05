//! N1 (the argument builder): one `thread spawn` per new thread, with
//! `--project`, `--machine cloud`, `--provider`, `--model`,
//! `--reasoning-level`, `--request-id`, `--json`, and the text as one
//! `--prompt` argument, byte for byte. Never an excluded flag, never an
//! omitted setting. The end-to-end half (through the runner and the fake)
//! is phase 2's.

use openwiz_adapter_cloudroom::new_thread::spawn_argv;
use openwiz_core::new_thread::operation::SpawnRequest;
use openwiz_core::new_thread::request_id;
use openwiz_core::prompt::ReadyPrompt;

use crate::support::*;

pub const EXCLUDED_SPAWN_FLAGS: &[&str] = &[
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

fn spawn(text: &str) -> SpawnRequest {
    SpawnRequest {
        project_id: "prj_syn_0001".to_owned(),
        provider: "codex".to_owned(),
        model: "syn-codex-model-01".to_owned(),
        effort: "high".to_owned(),
        request_id: request_id(&op("op-n01")),
        prompt: ReadyPrompt::text(text),
    }
}

#[test]
fn the_arguments_are_exactly_the_verified_shape() {
    let s = spawn("Synthetic prompt");
    let argv = spawn_argv(&s).expect("valid");
    let expected: Vec<String> = [
        "thread",
        "spawn",
        "--project",
        "prj_syn_0001",
        "--machine",
        "cloud",
        "--provider",
        "codex",
        "--model",
        "syn-codex-model-01",
        "--reasoning-level",
        "high",
        "--request-id",
        &s.request_id,
        "--json",
        "--prompt",
        "Synthetic prompt",
    ]
    .iter()
    .map(|w| w.to_string())
    .collect();
    assert_eq!(argv, expected);
}

#[test]
fn every_setting_is_explicit_once() {
    let argv = spawn_argv(&spawn("Synthetic")).unwrap();
    let flags = &argv[..argv.len() - 1];
    for flag in [
        "--project",
        "--machine",
        "--provider",
        "--model",
        "--reasoning-level",
        "--request-id",
        "--json",
        "--prompt",
    ] {
        assert_eq!(
            flags.iter().filter(|w| *w == flag).count(),
            1,
            "{flag} once in {argv:?}"
        );
    }
    let machine = flags.iter().position(|w| w == "--machine").unwrap();
    assert_eq!(flags[machine + 1], "cloud");
}

#[test]
fn the_text_is_one_last_argument_byte_for_byte() {
    for text in [
        "-leading dash",
        "--json",
        "--file /synthetic/a.png",
        "Line one\nLine two\r\n\ttabbed",
        "Synthétique 合成 🧪 \u{200b}",
        "\"quoted\" 'single' $HOME `tick` ; && |",
        " Synthetic padded ",
        "--",
    ] {
        let argv = spawn_argv(&spawn(text)).unwrap();
        assert_eq!(argv.last().map(String::as_str), Some(text));
        assert_eq!(argv[argv.len() - 2], "--prompt", "{text:?}");
        assert_eq!(argv.len(), 17, "{text:?}: one argument");
    }
}

#[test]
fn no_excluded_flag_is_ever_passed() {
    let argv = spawn_argv(&spawn("--title Synthetic --parent-self")).unwrap();
    let flags = &argv[..argv.len() - 1];
    for excluded in EXCLUDED_SPAWN_FLAGS {
        assert!(
            !flags
                .iter()
                .any(|w| w == excluded || w.starts_with(&format!("{excluded}="))),
            "{excluded} in {argv:?}"
        );
    }
    for must in [
        "--file",
        "--image",
        "--title",
        "--base-branch",
        "--parent-self",
    ] {
        assert!(EXCLUDED_SPAWN_FLAGS.contains(&must), "{must} is listed");
    }
}

#[test]
fn an_unsafe_value_is_refused_not_passed() {
    let mut bad = Vec::new();
    let mut s = spawn("Synthetic");
    s.project_id = "-prj".to_owned();
    bad.push(s);
    let mut s = spawn("Synthetic");
    s.provider = "--file".to_owned();
    bad.push(s);
    let mut s = spawn("Synthetic");
    s.model = "syn model".to_owned();
    bad.push(s);
    let mut s = spawn("Synthetic");
    s.effort = String::new();
    bad.push(s);
    let mut s = spawn("Synthetic");
    s.request_id = "ow1/../x".to_owned();
    bad.push(s);
    let mut s = spawn("Synthetic");
    s.request_id = "a".repeat(65);
    bad.push(s);
    for s in bad {
        assert!(spawn_argv(&s).is_err(), "{s:?}");
    }
}

#[test]
fn the_core_passes_the_chosen_settings_and_derived_request_id() {
    let client = FakeClient::new();
    let store = MemoryStore::new();
    let r = request(
        settings("claude-code", "syn-claude-model-03", "max"),
        "Synthetic",
        "op-n01-core",
    );
    create(&client, &store, &r);
    assert_eq!(client.spawns(), 1);
    let argv = client.last_argv();
    let value = |flag: &str| {
        let i = argv.iter().position(|w| w == flag).unwrap();
        argv[i + 1].clone()
    };
    assert_eq!(value("--provider"), "claude-code");
    assert_eq!(value("--model"), "syn-claude-model-03");
    assert_eq!(value("--reasoning-level"), "max");
    assert_eq!(value("--project"), "prj_syn_0001");
    assert_eq!(value("--request-id"), request_id(&op("op-n01-core")));
}
