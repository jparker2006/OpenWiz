use std::process::Command;

use openwiz_adapter_cloudroom::allowlist::{Invocation, SpawnArgs};
use openwiz_core::messaging::MAX_TEXT_BYTES;

use crate::fake::*;
use crate::n01_end_to_end::NEVER;

const RID: &str = "ow1-0123456789abcdef0123456789abcdef01234567";

fn args(
    project: &str,
    provider: &str,
    model: &str,
    effort: &str,
    rid: &str,
    prompt: &str,
) -> Result<SpawnArgs, String> {
    SpawnArgs::new(project, provider, model, effort, rid, prompt).map_err(|e| e.0)
}

#[test]
fn the_two_additions_in_exactly_their_shapes() {
    assert_eq!(
        Invocation::CloudStatus.argv(),
        ["cloud", "status", "--json"]
    );
    let spawn = args(
        "prj_syn_0001",
        "codex",
        "syn-codex-model-01",
        "high",
        RID,
        "-Synthetic",
    )
    .unwrap();
    assert_eq!(
        Invocation::ThreadSpawn(spawn).argv(),
        [
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
            RID,
            "--json",
            "--prompt",
            "-Synthetic",
        ]
    );
}

#[test]
fn a_spawn_can_only_be_built_with_safe_arguments() {
    let ok = (
        "prj_syn_0001",
        "codex",
        "syn-codex-model-01",
        "high",
        RID,
        "Synthetic",
    );
    assert!(args(ok.0, ok.1, ok.2, ok.3, ok.4, ok.5).is_ok());
    assert!(args(ok.0, "claude-code", ok.2, "max", ok.4, ok.5).is_ok());
    let cap = "a".repeat(MAX_TEXT_BYTES + 1);
    for (what, bad) in [
        ("project", args("-x", ok.1, ok.2, ok.3, ok.4, ok.5)),
        ("project", args("prj syn", ok.1, ok.2, ok.3, ok.4, ok.5)),
        ("project", args("", ok.1, ok.2, ok.3, ok.4, ok.5)),
        ("provider", args(ok.0, "pi", ok.2, ok.3, ok.4, ok.5)),
        ("provider", args(ok.0, "acp-cursor", ok.2, ok.3, ok.4, ok.5)),
        ("provider", args(ok.0, "--provider", ok.2, ok.3, ok.4, ok.5)),
        ("model", args(ok.0, ok.1, "-m", ok.3, ok.4, ok.5)),
        ("model", args(ok.0, ok.1, "syn model", ok.3, ok.4, ok.5)),
        ("model", args(ok.0, ok.1, "", ok.3, ok.4, ok.5)),
        (
            "model",
            args(ok.0, ok.1, &"m".repeat(129), ok.3, ok.4, ok.5),
        ),
        ("effort", args(ok.0, ok.1, ok.2, "ultra", ok.4, ok.5)),
        ("effort", args(ok.0, ok.1, ok.2, "none", ok.4, ok.5)),
        ("effort", args(ok.0, ok.1, ok.2, "", ok.4, ok.5)),
        ("request", args(ok.0, ok.1, ok.2, ok.3, "syn-request", ok.5)),
        ("request", args(ok.0, ok.1, ok.2, ok.3, "ow1-0123", ok.5)),
        (
            "request",
            args(ok.0, ok.1, ok.2, ok.3, &RID.to_uppercase(), ok.5),
        ),
        ("text", args(ok.0, ok.1, ok.2, ok.3, ok.4, "")),
        ("text", args(ok.0, ok.1, ok.2, ok.3, ok.4, " \n")),
        ("text", args(ok.0, ok.1, ok.2, ok.3, ok.4, "a\0b")),
        ("text", args(ok.0, ok.1, ok.2, ok.3, ok.4, &cap)),
    ] {
        let why = bad.expect_err(what);
        assert!(!why.is_empty());
    }
}

/// The fake's own copy of the allowlist refuses every other shape (exit
/// 97), and counts no refused call.
#[test]
fn the_fake_refuses_every_other_spawn_and_cloud_shape() {
    let fake = NewFake::new("n16_fake_refuses");
    let good = spawn_argv(PROJECT, "op-n16", "Synthetic");
    let mut refused: Vec<Vec<String>> = vec![
        vec!["cloud".into(), "status".into()],
        vec![
            "cloud".into(),
            "status".into(),
            "--json".into(),
            "--hidden".into(),
        ],
        vec!["cloud".into(), "retry-start".into(), "thr_syn_0001".into()],
        vec!["thread".into(), "spawn".into(), "--json".into()],
        good[..16].to_vec(),
    ];
    // Each flag `spawn --help` lists that OpenWiz never passes, added.
    for flag in NEVER {
        let mut argv = good.clone();
        argv.insert(14, (*flag).to_owned());
        argv.insert(15, "x".to_owned());
        refused.push(argv);
    }
    // Each value swapped for one outside the shape.
    for (i, value) in [
        (3, "-prj"),
        (5, "local"),
        (7, "pi"),
        (9, "-m"),
        (11, "ultra"),
        (13, "syn-request"),
    ] {
        let mut argv = good.clone();
        argv[i] = value.to_owned();
        refused.push(argv);
    }
    // Two settings swapped in order, or one dropped.
    let mut swapped = good.clone();
    swapped.swap(2, 6);
    swapped.swap(3, 7);
    refused.push(swapped);
    for drop in [2, 4, 6, 8, 10, 12, 14] {
        let mut argv = good.clone();
        argv.drain(drop..if drop == 14 { 15 } else { drop + 2 });
        refused.push(argv);
    }
    for argv in &refused {
        let out = Command::new(fake.program())
            .args(argv)
            .output()
            .expect("run the fake");
        assert_eq!(out.status.code(), Some(97), "{argv:?}");
    }
    assert_eq!(fake.spawn_count(), 0, "a refused call is not counted");
    let out = Command::new(fake.program()).args(&good).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "the one shape is accepted");
    assert_eq!(fake.spawn_count(), 1);
}
