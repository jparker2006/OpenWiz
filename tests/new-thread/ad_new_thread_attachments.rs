use std::collections::BTreeMap;
use std::fs;
use std::process::Output;
use std::time::Duration;

use openwiz_adapter_cloudroom::allowlist::{Invocation, SpawnArgs, ThreadId};
use openwiz_adapter_cloudroom::new_thread::spawn_argv as adapter_spawn_argv;
use openwiz_core::attachment::{Claim, PromptAttachment, StagedAttachment, StagedPath};
use openwiz_core::messaging::Mode;
use openwiz_core::model::Timestamp;
use openwiz_core::new_thread::operation::{
    NewThreadError, NewThreadReport, NewThreadResult, SpawnRequest, new_thread_with,
};
use openwiz_core::new_thread::outcome::{Outcome, Status};
use openwiz_core::new_thread::read::StrictJson;
use openwiz_core::new_thread::record::NewThreadRecord;
use openwiz_core::new_thread::{Binding, BindingField, NewThreadRequest, ProjectRef, request_id};
use openwiz_core::prompt::{NOT_REPORTED, ReadyPrompt, attachments_digest};
use openwiz_store::attachments::{Claims, ReadyAttachment};
use serde_json::{Value, json};

use crate::fake::*;
use crate::staging::{self, copy_path, id_of, kept, png, stage, text, uploads};
use crate::support::{T0, clock, offered, request};

const HOUR_MS: i64 = 60 * 60 * 1000;

fn with_ids(mut r: NewThreadRequest, ready: &[&ReadyAttachment]) -> NewThreadRequest {
    r.prompt.attachments = ready.iter().map(|a| a.attachment.id.clone()).collect();
    r.project = ProjectRef::Id(PROJECT.to_owned());
    r.budget = Duration::from_secs(3);
    r
}

fn create_with(fake: &NewFake, r: &NewThreadRequest) -> Result<NewThreadReport, NewThreadError> {
    new_thread_with(
        &fake.source(),
        &fake.store(),
        &staging::store(&fake.state),
        &clock(),
        r,
    )
}

fn created_with(fake: &NewFake, r: &NewThreadRequest) -> NewThreadReport {
    create_with(fake, r).unwrap_or_else(|e| panic!("create failed: {e:?}"))
}

fn outcome_of(report: &NewThreadReport) -> &Outcome {
    let NewThreadResult::Outcome { outcome, .. } = &report.result else {
        panic!("no outcome: {report:?}")
    };
    outcome
}

fn bound(ready: &[&ReadyAttachment]) -> Vec<PromptAttachment> {
    ready.iter().map(|a| (&a.attachment).into()).collect()
}

fn claim_state(fake: &NewFake, ready: &ReadyAttachment, now: Timestamp) -> Claim {
    staging::store(&fake.state)
        .recorded_claims(now)
        .claim(&ready.attachment.id)
        .unwrap()
}

/// The prefix of OpenWiz's `spawn` for `op_id`, up to and with `--json`.
fn spawn_prefix(op_id: &str) -> Vec<String> {
    let mut argv = spawn_argv(PROJECT, op_id, "x");
    argv.truncate(argv.len() - 2);
    argv
}

// AD1: the exact invocation, the same tail as tell's.

#[test]
fn ad01_one_spawn_carries_every_staged_copy_before_the_prompt() {
    let fake = NewFake::new("ad01_spawn");
    let a = stage(&fake.state, "syn-ad01-a.png", &png(2048), T0);
    let notes = stage(&fake.state, "syn-ad01-notes.txt", &text("notes\n"), T0);
    let r = with_ids(
        request(offered(), "Synthetic first prompt", "op-ad01-new"),
        &[&a, &notes],
    );
    let report = created_with(&fake, &r);
    assert_eq!(report.status, Status::Created, "{report:?}");
    let spawns = fake.spawns();
    assert_eq!(spawns.len(), 1);
    let path = |r: &ReadyAttachment| copy_path(&fake.state, r).to_str().unwrap().to_owned();
    let mut expected = spawn_prefix("op-ad01-new");
    expected.extend([
        "--file".to_owned(),
        path(&notes),
        "--image".to_owned(),
        path(&a),
        "--prompt".to_owned(),
        "Synthetic first prompt".to_owned(),
    ]);
    assert_eq!(spawns[0], expected);
    let log = uploads(&fake.dir);
    assert_eq!(log.len(), 1);
    assert_eq!(log[0]["request_id"], request_id(&r.op_id));
    for (item, r) in log[0]["items"].as_array().unwrap().iter().zip([&notes, &a]) {
        assert_eq!(item["sha256"], r.attachment.sha256.as_str());
    }
    assert_eq!(report.attachments, bound(&[&a, &notes]));
    assert_eq!(
        report.attachments_digest,
        attachments_digest(&bound(&[&a, &notes]))
    );
    let words = outcome_of(&report).text();
    assert!(
        words.contains("with 2 attachments handed to CloudRoom") && words.contains(NOT_REPORTED),
        "{words}"
    );
}

#[test]
fn ad01_tell_and_spawn_get_the_same_tail_from_attachment_args() {
    let area = std::path::Path::new("/synthetic/state/attachments");
    let staged = |id: &str, name: &str, kind| {
        let id = staging::attachment_id(id);
        let name = openwiz_core::attachment::SafeName::parse(name).unwrap();
        StagedAttachment {
            path: StagedPath::new(area, &id, &name).unwrap(),
            attachment: PromptAttachment {
                id,
                sha256: openwiz_core::attachment::Sha256::parse(&"a".repeat(64)).unwrap(),
                size_bytes: 1,
                kind,
                name,
            },
        }
    };
    let files = vec![
        staged(
            "att-syn-1",
            "syn-notes.txt",
            openwiz_core::attachment::Kind::File,
        ),
        staged(
            "att-syn-2",
            "syn-photo.png",
            openwiz_core::attachment::Kind::Image,
        ),
    ];
    let tell = Invocation::ThreadTell {
        id: ThreadId::parse("thr_syn_0001").unwrap(),
        mode: Mode::Queue,
        text: "Synthetic".to_owned(),
        attachments: files.clone(),
    }
    .argv();
    let spawn = Invocation::ThreadSpawn(
        SpawnArgs::new(
            PROJECT,
            "codex",
            "syn-codex-model-01",
            "high",
            &request_id(&crate::support::op("op-ad01-tail")),
            "Synthetic",
        )
        .unwrap()
        .with_attachments(files),
    )
    .argv();
    let tail = [
        "--file",
        "/synthetic/state/attachments/att-syn-1/syn-notes.txt",
        "--image",
        "/synthetic/state/attachments/att-syn-2/syn-photo.png",
    ];
    assert_eq!(&tell[6..10], tail, "{tell:?}");
    assert_eq!(&tell[10..], ["--", "Synthetic"]);
    assert_eq!(&spawn[15..19], tail, "{spawn:?}");
    assert_eq!(&spawn[19..], ["--prompt", "Synthetic"]);
}

#[test]
fn ad01_no_relative_token_file_url_or_unplain_path_can_become_a_staged_path() {
    let id = staging::attachment_id("att-syn-1");
    let name = openwiz_core::attachment::SafeName::parse("syn.txt").unwrap();
    for area in [
        "attachments",
        "relative/attachments",
        "file:///synthetic/attachments",
        "/synthetic/../attachments",
        "/synthetic/./attachments",
        "/synthetic/elsewhere",
        "/synthetic/line\nbreak/attachments",
    ] {
        assert!(
            StagedPath::new(std::path::Path::new(area), &id, &name).is_err(),
            "{area:?}"
        );
    }
    assert_eq!(
        StagedPath::new(std::path::Path::new("/synthetic/attachments"), &id, &name)
            .unwrap()
            .as_str(),
        "/synthetic/attachments/att-syn-1/syn.txt"
    );
    // A text-only prompt passes no flag at all.
    let spawn = SpawnRequest {
        project_id: PROJECT.to_owned(),
        provider: "codex".to_owned(),
        model: "syn-codex-model-01".to_owned(),
        effort: "high".to_owned(),
        request_id: request_id(&crate::support::op("op-ad01-none")),
        prompt: ReadyPrompt::text("--file /synthetic/x.txt"),
    };
    let argv = adapter_spawn_argv(&spawn).unwrap();
    assert_eq!(&argv[15..], ["--prompt", "--file /synthetic/x.txt"]);
}

// AD4: the binding, for a new thread.

#[test]
fn ad04_same_op_id_and_payload_is_one_spawn_any_attachment_difference_is_refused() {
    let fake = NewFake::new("ad04_new_binding");
    let a = stage(&fake.state, "syn-ad04-a.txt", &text("a\n"), T0);
    let b = stage(&fake.state, "syn-ad04-b.png", &png(1024), T0);
    let c = stage(&fake.state, "syn-ad04-c.txt", &text("c\n"), T0);
    let a2 = stage(&fake.state, "syn-ad04-a.txt", &text("A\n"), T0);
    let base = with_ids(request(offered(), "Synthetic", "op-ad04-new"), &[&a, &b]);
    assert_eq!(created_with(&fake, &base).status, Status::Created);
    let again = created_with(&fake, &base);
    assert!(matches!(
        again.result,
        NewThreadResult::Outcome { replayed: true, .. }
    ));
    for (name, ids) in [
        ("added", vec![&a, &b, &c]),
        ("removed", vec![&a]),
        ("reordered", vec![&b, &a]),
        ("changed bytes", vec![&a2, &b]),
        ("none", vec![]),
    ] {
        let report = created_with(&fake, &with_ids(base.clone(), &ids));
        assert_eq!(report.status, Status::ConflictingReuse, "{name}");
        let NewThreadResult::ConflictingReuse { differs } = &report.result else {
            panic!("{name}")
        };
        assert_eq!(differs, &vec![BindingField::Attachments], "{name}");
    }
    assert_eq!(fake.spawn_count(), 1, "no second spawn");
}

#[test]
fn ad04_an_a1_record_without_attachments_still_reads_and_matches() {
    let fake = NewFake::new("ad04_a1_record");
    let r = with_ids(request(offered(), "Synthetic A1", "op-ad04-a1"), &[]);
    assert_eq!(created_with(&fake, &r).status, Status::Created);
    let record = fake.record("op-ad04-a1");
    assert!(record.get("attachments").is_none(), "absent means none");
    let again = created_with(&fake, &r);
    assert!(matches!(
        again.result,
        NewThreadResult::Outcome { replayed: true, .. }
    ));
    assert_eq!(fake.spawn_count(), 1);
}

#[test]
fn ad04_the_record_reader_takes_attachments_only_exactly_and_agreeing() {
    let r = with_ids(request(offered(), "Synthetic", "op-ad04-read"), &[]);
    let a = PromptAttachment {
        id: staging::attachment_id("att-syn-1"),
        sha256: openwiz_core::attachment::Sha256::parse(&"b".repeat(64)).unwrap(),
        size_bytes: 3,
        kind: openwiz_core::attachment::Kind::File,
        name: openwiz_core::attachment::SafeName::parse("syn.txt").unwrap(),
    };
    let record = NewThreadRecord::claimed(
        r.op_id.clone(),
        Binding::new(&r, attachments_digest(std::slice::from_ref(&a))),
        r.settings.clone(),
        "Synthetic",
        T0,
    )
    .with_attachments(vec![a]);
    let good = serde_json::to_value(&record).unwrap();
    let read = |v: &Value| {
        let tree: StrictJson = serde_json::from_str(&v.to_string()).unwrap();
        NewThreadRecord::read(&tree)
    };
    assert_eq!(read(&good).unwrap(), record);
    let mut bad = Vec::new();
    let mut v = good.clone();
    v["attachments"] = json!([]);
    bad.push(("empty", v));
    let mut v = good.clone();
    v["attachments"][0]["extra"] = json!(1);
    bad.push(("extra key", v));
    let mut v = good.clone();
    v["attachments"][0].as_object_mut().unwrap().remove("name");
    bad.push(("missing key", v));
    let mut v = good.clone();
    v["attachments"][0]["kind"] = json!("photo");
    bad.push(("unknown kind", v));
    let mut v = good.clone();
    v["attachments"][0]["size_bytes"] = json!(4);
    bad.push(("not the bound attachments", v));
    let mut v = good.clone();
    v.as_object_mut().unwrap().remove("attachments");
    bad.push(("absent while the digest says some", v));
    let mut v = good.clone();
    let first = v["attachments"][0].clone();
    v["attachments"] = json!([first.clone(), first]);
    bad.push(("an id twice", v));
    for (name, v) in &bad {
        assert!(read(v).is_err(), "{name}");
        assert!(
            !crate::support::validate_def("record", v).is_empty()
                || *name == "not the bound attachments"
                || *name == "absent while the digest says some",
            "{name}: the schema took it"
        );
    }
    assert_eq!(
        crate::support::validate_def("record", &good),
        Vec::<String>::new()
    );
}

// AD6: one request id never carries two payloads.

fn new_with<'a>(op: &'a str, extra: &[&'a str], json: bool) -> Vec<&'a str> {
    let mut args = new_args_by_id(PROJECT, op, extra, "Synthetic launch");
    if json {
        args.insert(args.len() - 2, "--json");
    }
    args
}

fn exit_of(output: &Output) -> Option<i32> {
    output.status.code()
}

#[test]
fn ad06_across_processes_and_restarts_no_request_id_carries_two_payloads() {
    let fake = NewFake::with(
        "ad06_request_ids",
        vec![
            json!({"any_spawn": true, "kill_parent": true, "creates": true, "stdout_file": file("spawn-receipt-created.json")}),
        ],
    );
    // Release processes: their clock is the real one.
    let now = staging::now();
    let a = stage(&fake.state, "syn-ad06-a.txt", &text("a\n"), now);
    let b = stage(&fake.state, "syn-ad06-b.txt", &text("b\n"), now);
    let (ida, idb) = (id_of(&a), id_of(&b));
    // The create is killed during its spawn: uncertain.
    let first = fake.run_process(&new_with("op-ad06", &["--attachment", &ida], false), "");
    assert_eq!(exit_of(&first), None, "killed");
    assert_eq!(fake.spawn_count(), 1);
    fake.respond(vec![]);
    // Again with other attachments, as a new process: refused, no spawn.
    let (code, report) =
        json_of(&fake.run_process(&new_with("op-ad06", &["--attachment", &idb], true), ""));
    assert_eq!(report["status"], "conflicting_reuse", "{report}");
    assert_ne!(code, Some(0));
    // Again with the same: the recovered uncertain outcome, no spawn.
    let (_, report) =
        json_of(&fake.run_process(&new_with("op-ad06", &["--attachment", &ida], true), ""));
    assert_eq!(report["status"], "uncertain", "{report}");
    assert_eq!(report["result"]["replayed"], true);
    // The status read: zero writes.
    let before = fake.writes().len();
    let status = fake.run_process(&["new", "--status", "op-ad06", "--json"], "");
    assert_eq!(exit_of(&status), Some(6));
    assert_eq!(fake.writes().len(), before, "a status read writes nothing");
    assert_eq!(fake.spawn_count(), 1, "one spawn in all");
    assert!(kept(&fake.state, &a) && kept(&fake.state, &b));
    // No request id was ever passed with two attachment sets.
    let mut seen: BTreeMap<String, Value> = BTreeMap::new();
    for line in uploads(&fake.dir) {
        let rid = line["request_id"].as_str().unwrap().to_owned();
        let hashes: Vec<Value> = line["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["sha256"].clone())
            .collect();
        let hashes = Value::from(hashes);
        let known = seen.entry(rid).or_insert_with(|| hashes.clone());
        assert_eq!(*known, hashes, "one request id, two payloads");
    }
    // There is no retry: no form of `new` re-sends.
    let (code, _, err) = fake.cli(&["new", "--retry", "op-ad06"], "");
    assert_eq!(code, 2, "{err}");
}

// AD7: the outcome table, for spawn.

#[test]
fn ad07_every_row_of_the_outcome_table_for_spawn() {
    for (case, response, status) in [
        ("created", created(), "created"),
        (
            "documented refusal",
            json!({"any_spawn": true, "exit": 1, "stderr": "Error: Failed to create thread: HTTP 400: Message is too long"}),
            "refused",
        ),
        (
            "uploaded then no answer",
            json!({"any_spawn": true, "hang": true}),
            "uncertain",
        ),
        (
            "upload fails part way",
            json!({"any_spawn": true, "upload_fails_after": 1, "stdout_file": file("spawn-receipt-created.json")}),
            "uncertain",
        ),
        (
            "an attachment refusal no fixture documents",
            json!({"any_spawn": true, "exit": 1, "stderr": "Error: Failed to create thread: HTTP 400: Cloud attachments are not enabled"}),
            "uncertain",
        ),
    ] {
        let fake = NewFake::with(&format!("ad07_{}", case.replace(' ', "_")), vec![response]);
        let a = stage(&fake.state, "syn-ad07-a.txt", &text("a\n"), T0);
        let b = stage(&fake.state, "syn-ad07-b.png", &png(1024), T0);
        let r = with_ids(request(offered(), "Synthetic", "op-ad07"), &[&a, &b]);
        let report = created_with(&fake, &r);
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["status"], status, "{case}: {json}");
        let handed = outcome_of(&report).text().contains("handed to CloudRoom");
        assert_eq!(handed, status == "created", "{case}");
        assert_eq!(fake.spawn_count(), 1, "{case}");
    }
}

// AD9: retention, for a new thread.

#[test]
fn ad09_retention_follows_the_table_for_a_new_thread() {
    let later = Timestamp(T0.0 + 25 * HOUR_MS);
    let fake = NewFake::new("ad09_created");
    let a = stage(&fake.state, "syn-ad09-a.txt", &text("a\n"), T0);
    let report = created_with(
        &fake,
        &with_ids(request(offered(), "S", "op-ad09-created"), &[&a]),
    );
    assert_eq!(report.status, Status::Created);
    assert_eq!(
        uploads(&fake.dir)[0]["items"][0]["sha256"],
        a.attachment.sha256.as_str()
    );
    assert!(!kept(&fake.state, &a), "a created thread's copy is deleted");

    let fake = NewFake::with(
        "ad09_refused",
        vec![
            json!({"any_spawn": true, "exit": 1, "stderr": "Error: Failed to create thread: HTTP 400: Message is too long"}),
        ],
    );
    let b = stage(&fake.state, "syn-ad09-b.txt", &text("b\n"), T0);
    let report = created_with(
        &fake,
        &with_ids(request(offered(), "S", "op-ad09-refused"), &[&b]),
    );
    assert_eq!(report.status, Status::Refused);
    assert!(kept(&fake.state, &b));
    assert_eq!(claim_state(&fake, &b, T0), Claim::Failed);
    assert_eq!(
        staging::claim_of(&fake.state, &b),
        Some(json!({"kind": "new_thread", "op_id": "op-ad09-refused"}))
    );
    let store = staging::store(&fake.state);
    store.sweep(later, &store.recorded_claims(later)).unwrap();
    assert!(!kept(&fake.state, &b), "expired at the time-to-live");

    let fake = NewFake::with(
        "ad09_uncertain",
        vec![json!({"any_spawn": true, "exit": 1, "stderr": "Error: synthetic undocumented"})],
    );
    let c = stage(&fake.state, "syn-ad09-c.txt", &text("c\n"), T0);
    let report = created_with(
        &fake,
        &with_ids(request(offered(), "S", "op-ad09-uncertain"), &[&c]),
    );
    assert_eq!(report.status, Status::Uncertain);
    let store = staging::store(&fake.state);
    store.sweep(later, &store.recorded_claims(later)).unwrap();
    assert!(kept(&fake.state, &c), "kept until resolved");
    assert_eq!(claim_state(&fake, &c, later), Claim::Uncertain);
}

// AD10: crash recovery, for a new thread.

#[test]
fn ad10_a_crash_at_each_step_keeps_the_attachments_and_creates_nothing_more() {
    for (crash, recovered, claim) in [
        ("after-claim", "failed", None),
        (
            "after-dispatching-mark",
            "uncertain",
            Some(Claim::Uncertain),
        ),
        ("after-call", "uncertain", Some(Claim::Uncertain)),
    ] {
        let fake = NewFake::new(&format!("ad10_new_{crash}"));
        // The harness runs on the real clock.
        let now = staging::now();
        let a = stage(
            &fake.state,
            &format!("syn-ad10-{crash}.txt"),
            &text("a\n"),
            now,
        );
        let id = id_of(&a);
        let op = format!("op-ad10-{crash}");
        let (mut command, _) = fake.harness_with(&op, crash, "Synthetic", &[&id]);
        let output = command.output().expect("run the harness");
        assert!(!output.status.success(), "{crash}: the harness stopped");
        let spawns = fake.spawn_count();
        // `new --status`, then the same create: nothing more is created.
        let (_, status) = fake.cli_json(&["new", "--status", &op], "");
        assert_eq!(
            status["result"]["report"]["status"], recovered,
            "{crash}: {status}"
        );
        // The harness's own create: its project, settings and text.
        let args = new_args(
            "Synthetic project one",
            &op,
            &["--attachment", &id],
            "Synthetic",
        );
        let (_, report) = fake.cli_json(&args, "");
        assert_eq!(report["status"], recovered, "{crash}: {report}");
        assert_eq!(
            fake.spawn_count(),
            spawns,
            "{crash}: nothing dispatched again"
        );
        assert!(kept(&fake.state, &a), "{crash}: kept");
        let state = claim_state(&fake, &a, T0);
        match claim {
            Some(want) => assert_eq!(state, want, "{crash}"),
            // Stopped before its attachments were claimed: unclaimed, and
            // the failed op claims nothing.
            None => assert!(
                matches!(state, Claim::Unclaimed | Claim::Failed),
                "{crash}: {state:?}"
            ),
        }
    }
}

// AD13: the CLI's new.

#[test]
fn ad13_new_with_attach_and_attachment_creates_once_and_the_json_matches_the_text() {
    let fake = NewFake::new("ad13_new");
    let staged = stage(&fake.state, "syn-ad13-photo.png", &png(2048), T0);
    let dir = fake.dir.join("sources");
    crate::dirs::private_dir_all(&dir).unwrap();
    let notes = dir.join("syn-ad13-notes.txt");
    fs::write(&notes, "Synthetic notes.\n").unwrap();
    let id = id_of(&staged);
    let extra = [
        "--attachment",
        id.as_str(),
        "--attach",
        notes.to_str().unwrap(),
    ];
    let (code, out, err) = fake.cli(&new_with("op-ad13-new", &extra, false), "");
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(fake.spawn_count(), 1);
    let (code, report) = fake.cli_json(&new_with("op-ad13-new", &extra, false), "");
    assert_eq!(code, 0);
    assert_eq!(
        crate::support::validate_def("report", &report),
        Vec::<String>::new(),
        "{report}"
    );
    let attachments = report["attachments"].as_array().unwrap();
    assert_eq!(attachments.len(), 2);
    assert_eq!(attachments[0]["id"], id.as_str());
    for a in attachments {
        let line = format!(
            "attachment: {} · {} · {} · {} bytes · SHA-256 {}",
            a["id"].as_str().unwrap(),
            a["kind"].as_str().unwrap(),
            a["name"].as_str().unwrap(),
            a["size_bytes"],
            a["sha256"].as_str().unwrap()
        );
        assert!(out.contains(&line), "{line}\n{out}");
    }
    assert!(out.contains(NOT_REPORTED), "{out}");
    assert_eq!(fake.spawn_count(), 1, "the second run replayed");
}

#[test]
fn ad13_the_preview_stages_and_prints_the_command_with_each_attachment() {
    let fake = NewFake::new("ad13_preview");
    let dir = fake.dir.join("sources");
    crate::dirs::private_dir_all(&dir).unwrap();
    let notes = dir.join("syn-ad13-preview.txt");
    fs::write(&notes, "Synthetic.\n").unwrap();
    let mut args = new_with(
        "op-ad13-preview",
        &["--attach", notes.to_str().unwrap()],
        false,
    );
    args.retain(|a| *a != "--yes");
    let (code, out, err) = fake.cli(&args, "");
    assert_eq!(code, 12, "{out}{err}");
    assert_eq!(fake.spawn_count(), 0, "nothing was created");
    let listed = staging::store(&fake.state)
        .list(&staging::store(&fake.state).recorded_claims(T0))
        .unwrap();
    assert_eq!(listed.len(), 1, "staged on this machine only");
    let openwiz_store::attachments::Listed::Ready { attachment, .. } = &listed[0] else {
        panic!("{listed:?}")
    };
    let id = attachment.attachment.id.as_str();
    let command = out
        .lines()
        .find(|l| {
            l.trim_start().starts_with("openwiz new ") && l.ends_with("--yes -- 'Synthetic launch'")
        })
        .unwrap_or_else(|| panic!("no command: {out}"));
    assert!(
        command.contains(&format!("--attachment '{id}'")),
        "{command}"
    );
    assert!(!command.contains("--attach '"), "{command}");
}

#[test]
fn ad13_attach_that_fails_on_any_path_creates_nothing() {
    let fake = NewFake::new("ad13_new_fails");
    let dir = fake.dir.join("sources");
    crate::dirs::private_dir_all(&dir).unwrap();
    let good = dir.join("syn-ad13-good.txt");
    fs::write(&good, "good\n").unwrap();
    let missing = dir.join("syn-ad13-missing.txt");
    let extra = [
        "--attach",
        good.to_str().unwrap(),
        "--attach",
        missing.to_str().unwrap(),
    ];
    let (code, out, err) = fake.cli(&new_with("op-ad13-fails", &extra, false), "");
    assert_eq!(code, 5, "{out}{err}");
    assert!(
        out.contains("syn-ad13-good.txt") && out.contains("syn-ad13-missing.txt"),
        "{out}"
    );
    assert!(fake.writes().is_empty(), "nothing was created");
    assert!(fake.invocations().is_empty(), "nothing was read");
}

#[test]
fn ad13_the_printed_command_attaches_exactly_the_previewed_copy() {
    use crate::n18_cli::{printed_command, run_printed, shells};
    let fake = NewFake::new("ad13_printed");
    let dir = fake.dir.join("sources");
    crate::dirs::private_dir_all(&dir).unwrap();
    let notes = dir.join("syn-ad13-printed.txt");
    fs::write(&notes, "Synthetic printed.\n").unwrap();
    // The release binary throughout: its clock is the real one.
    let mut args = new_with(
        "op-ad13-printed",
        &["--attach", notes.to_str().unwrap()],
        false,
    );
    args.retain(|a| *a != "--yes");
    let preview = fake.run_process(&args, "");
    assert_eq!(preview.status.code(), Some(12));
    let out = String::from_utf8(preview.stdout).unwrap();
    let command = printed_command(&out);
    let listed = staging::store(&fake.state)
        .list(&staging::store(&fake.state).recorded_claims(staging::now()))
        .unwrap();
    let [openwiz_store::attachments::Listed::Ready { attachment, .. }] = listed.as_slice() else {
        panic!("one staged copy: {listed:?}")
    };
    assert!(command.contains(&format!(
        "--attachment '{}'",
        attachment.attachment.id.as_str()
    )));
    let mut shells = shells();
    // bash first: it creates; the others replay.
    shells.rotate_left(1);
    for shell in &shells {
        let run = run_printed(shell, &fake, &command);
        assert_eq!(
            run.status.code(),
            Some(0),
            "{shell:?}: {}{}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        );
    }
    assert_eq!(fake.spawn_count(), 1, "one create, then replays");
    let log = uploads(&fake.dir);
    assert_eq!(log.len(), 1);
    assert_eq!(
        log[0]["items"][0]["sha256"],
        attachment.attachment.sha256.as_str()
    );
    let record = fake.record("op-ad13-printed");
    assert_eq!(
        record["attachments"][0]["id"],
        attachment.attachment.id.as_str()
    );
}
