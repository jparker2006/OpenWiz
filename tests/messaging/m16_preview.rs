//! M16 Preview: what a send to one target in one mode would
//! do, before any text is written. It reads the target and returns `send`'s
//! effects and the read's time, with **no claim and no `tell`**. Its effects
//! equal `send`'s for every target in the send fixtures, because `send`
//! reaches them through the same function.
//!
//! The CLI form is `openwiz send <id> --preview [--steer] [--json]`: it
//! takes no text and never opens the send record.

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use openwiz_adapter_cloudroom::source::CloudRoomSource;
use openwiz_cli::send::{SendContext, random_op_id, run_send};
use openwiz_core::messaging::InvalidRequest;
use openwiz_core::messaging::effects::TargetRecord;
use openwiz_core::messaging::record::{SendStore, StoreError};
use openwiz_core::messaging::send::{self, Messenger, SendGap, TellReply};
use openwiz_core::model::{SessionId, Timestamp};
use openwiz_core::ops::Clock;
use openwiz_core::source::SourceError;

use openwiz_cli::send::{EXIT_DELIVERED, EXIT_SEND_FAILED};
use openwiz_cli::{EXIT_NOT_FOUND, EXIT_USAGE};
use openwiz_core::messaging::Mode;
use openwiz_core::messaging::preview::{PREVIEW_SCHEMA_ID, PreviewStatus, preview};
use openwiz_core::messaging::send::{Delivery, FailReason, SendResult};
use openwiz_core::model::Fact;
use serde_json::Value;

use crate::support::*;

/// Every target the send fixtures answer for: the normal scenario's and the
/// send scenario's, an archived one, a hidden one and a missing one.
const TARGETS: &[&str] = &[
    "thr_syn_0001",
    "thr_syn_0002",
    "thr_syn_0003",
    "thr_syn_0004",
    "thr_syn_0005",
    "thr_syn_0006",
    "thr_syn_0007",
    "thr_syn_0008",
    "thr_syn_0011",
    "thr_syn_0012",
    "thr_syn_0014",
    "thr_syn_0015",
    "thr_syn_9999",
];

/// The reads a preview may make: the allowlisted target reads.
fn is_target_read(argv: &[String]) -> bool {
    let words: Vec<&str> = argv.iter().map(String::as_str).collect();
    matches!(
        words.as_slice(),
        ["thread", "show", _, "--json"]
            | ["thread", "list", "--json"]
            | ["machine", "list", "--json"]
    )
}

#[test]
fn a_preview_makes_no_claim_and_no_tell() {
    let fake = SendFake::new("m16_no_claim", "send");
    for target in TARGETS {
        for mode in [Mode::Queue, Mode::Steer] {
            let report = preview(&fake.source(), &clock(), &sid(target), mode)
                .unwrap_or_else(|e| panic!("{target}: {e}"));
            assert_eq!(report.schema, PREVIEW_SCHEMA_ID);
            assert_eq!(report.target.id.0, *target);
            assert_eq!(report.requested_mode, mode);
            assert_eq!(report.read_at, T0, "{target}: the read's time");
            assert!(report.summary.contains("nothing was sent"), "{report:#?}");
        }
    }
    assert_eq!(fake.tell_count(), 0);
    assert!(fake.tells().is_empty());
    let calls = fake.invocations();
    assert!(!calls.is_empty());
    for argv in &calls {
        assert!(
            is_target_read(argv),
            "a preview made another call: {argv:?}"
        );
    }
    // The core's preview takes no store at all; the CLI never opens one.
    assert!(!fake.state.exists(), "a send record was made");
    for target in TARGETS {
        let _ = fake.cli(&[target, "--preview"], "");
        let _ = fake.cli(&[target, "--preview", "--steer", "--json"], "");
    }
    assert_eq!(fake.tell_count(), 0);
    assert!(
        !fake.state.exists(),
        "the CLI's preview opened the send record"
    );
    for argv in fake.invocations() {
        assert!(
            is_target_read(&argv),
            "a preview made another call: {argv:?}"
        );
    }
}

#[test]
fn a_preview_costs_two_or_three_reads() {
    let fake = SendFake::new("m16_cost", "send");
    // Cloud: thread show and thread list. Local: and the machine list.
    for (target, reads) in [("thr_syn_0001", 2), ("thr_syn_0002", 3)] {
        let before = fake.invocations().len();
        preview(&fake.source(), &clock(), &sid(target), Mode::Queue).unwrap();
        assert_eq!(fake.invocations().len() - before, reads, "{target}");
    }
}

/// The reason a send failed with, if it failed.
fn failed(result: &SendResult) -> Option<FailReason> {
    match result {
        SendResult::Outcome {
            delivery: Delivery::Failed { reason, .. },
            ..
        } => Some(*reason),
        _ => None,
    }
}

#[test]
fn a_previews_effects_equal_sends_for_every_send_fixture() {
    let fake = SendFake::new("m16_equal", "send");
    let mut i = 0;
    for target in TARGETS {
        for mode in [Mode::Queue, Mode::Steer] {
            i += 1;
            let shown = preview(&fake.source(), &clock(), &sid(target), mode).unwrap();
            // Every call is logged, even one the fake has no answer for.
            let tells = fake.tells().len();
            let mut r = request(target, mode, "Synthetic", &format!("op-m16-{i}"));
            r.accept_possible_duplicate = true;
            let sent = fake.report(&r);
            let called = fake.tells().len() > tells;
            let what = format!("{target} {mode:?}: {shown:#?}\n{sent:#?}");
            assert_eq!(shown.effects, sent.effects, "{what}");
            assert_eq!(shown.target.title, sent.target.title, "{what}");
            for gap in &shown.gaps {
                assert!(sent.gaps.contains(gap), "{what}");
            }
            match shown.status {
                PreviewStatus::Ready => {
                    assert!(shown.refusal.is_none(), "{what}");
                    assert!(called, "ready, yet the send made no call: {what}");
                }
                PreviewStatus::NeedsAcknowledgement => {
                    assert_eq!(
                        shown.refusal.as_ref().map(|r| r.reason),
                        Some(FailReason::PausedQueueNotAcknowledged),
                        "{what}"
                    );
                    assert!(shown.effects.as_ref().unwrap().needs_acknowledgement);
                    assert_eq!(
                        failed(&sent.result),
                        Some(FailReason::PausedQueueNotAcknowledged),
                        "{what}"
                    );
                    assert!(!called, "{what}");
                }
                PreviewStatus::Refused => {
                    let refusal = shown.refusal.as_ref().expect("a refusal");
                    assert_eq!(failed(&sent.result), Some(refusal.reason), "{what}");
                    assert!(shown.effects.is_none(), "{what}");
                    assert!(!called, "{what}");
                    // The same words as the send's failure.
                    let Some(SendResult::Outcome { delivery, .. }) = Some(&sent.result) else {
                        unreachable!()
                    };
                    assert_eq!(delivery.text(), format!("failed: {}", refusal.text));
                }
            }
        }
    }
    // Every status is covered by the fixtures.
    let status = |t: &str| {
        preview(&fake.source(), &clock(), &sid(t), Mode::Queue)
            .unwrap()
            .status
    };
    assert_eq!(status("thr_syn_0001"), PreviewStatus::Ready);
    assert_eq!(status("thr_syn_0011"), PreviewStatus::NeedsAcknowledgement);
    assert_eq!(status("thr_syn_0008"), PreviewStatus::Refused);
    assert_eq!(status("thr_syn_9999"), PreviewStatus::Refused);
}

#[test]
fn a_preview_names_each_refusal() {
    let fake = SendFake::new("m16_refusals", "send");
    for (target, reason) in [
        ("thr_syn_9999", FailReason::NotFound),
        ("thr_syn_0007", FailReason::TargetHidden),
        ("thr_syn_0008", FailReason::TargetArchived),
    ] {
        let report = preview(&fake.source(), &clock(), &sid(target), Mode::Queue).unwrap();
        assert_eq!(report.status, PreviewStatus::Refused, "{target}");
        assert_eq!(report.refusal.as_ref().unwrap().reason, reason, "{target}");
        assert!(report.summary.contains("would be refused"), "{report:#?}");
    }
    let report = preview(&fake.source(), &clock(), &sid("thr_syn_9999"), Mode::Queue).unwrap();
    assert_eq!(
        report.target.title,
        Fact::Unknown("not read".to_owned()),
        "{report:#?}"
    );
}

#[test]
fn an_invalid_target_id_is_refused_with_no_read() {
    let fake = SendFake::new("m16_invalid", "send");
    assert!(preview(&fake.source(), &clock(), &sid("-x"), Mode::Queue).is_err());
    let (code, out, err) = fake.cli(&["--preview", "--", "-x"], "");
    assert_eq!(code, EXIT_USAGE, "{out}{err}");
    assert!(fake.invocations().is_empty());
}

#[test]
fn the_cli_preview_matches_the_schema_and_the_text() {
    let fake = SendFake::new("m16_cli", "send");
    for target in TARGETS {
        for steer in [false, true] {
            let mut args = vec![*target, "--preview"];
            if steer {
                args.push("--steer");
            }
            let (code, json) = fake.cli_json(&args, "");
            let errors = validate_def("preview", &json);
            assert!(errors.is_empty(), "{target}: {errors:#?}\n{json:#}");
            assert_eq!(json["schema"], PREVIEW_SCHEMA_ID);
            assert_eq!(
                json["requested_mode"],
                if steer { "steer" } else { "queue" }
            );
            let expected = match json["status"].as_str().unwrap() {
                "ready" => EXIT_DELIVERED,
                _ if json["refusal"]["reason"] == "not_found" => EXIT_NOT_FOUND,
                _ => EXIT_SEND_FAILED,
            };
            assert_eq!(code, expected, "{target}: {json:#}");

            let (text_code, text, err) = fake.cli(&args, "");
            assert_eq!(text_code, code, "{target}: {err}");
            for wanted in wanted_preview(&json) {
                assert!(
                    text.contains(&wanted),
                    "{target}: {wanted:?} not in\n{text}"
                );
            }
        }
    }
    let (_, text, _) = fake.cli(&["thr_syn_0011", "--preview"], "");
    assert!(text.contains("--resume-paused-queue"), "{text}");
    assert_eq!(fake.tell_count(), 0);
}

/// What the text must contain for a JSON preview.
fn wanted_preview(json: &Value) -> Vec<String> {
    let mut wanted = vec![
        "preview".to_owned(),
        json["target"]["id"].as_str().unwrap().to_owned(),
        json["summary"].as_str().unwrap().to_owned(),
    ];
    if let Some(title) = json["target"]["title"]["known"].as_str() {
        wanted.push(title.to_owned());
    }
    let effects = &json["effects"];
    if !effects.is_null() {
        for key in ["effect", "paused_queue"] {
            wanted.push(match effects[key]["known"]["text"].as_str() {
                Some(text) => text.to_owned(),
                None => format!("unknown ({})", effects[key]["unknown"].as_str().unwrap()),
            });
        }
    }
    if let Some(text) = json["refusal"]["text"].as_str() {
        wanted.push(text.to_owned());
    }
    for gap in json["gaps"].as_array().unwrap() {
        wanted.push(gap["detail"].as_str().unwrap().to_owned());
    }
    wanted
}

#[test]
fn a_preview_takes_no_text_and_no_send_option() {
    let fake = SendFake::new("m16_usage", "send");
    for args in [
        vec!["thr_syn_0001", "--preview", "--", "Synthetic"],
        vec!["thr_syn_0001", "--preview", "Synthetic"],
        vec!["thr_syn_0001", "--preview", "--op-id", "op-m16-usage"],
        vec!["thr_syn_0001", "--preview", "--resume-paused-queue"],
        vec!["thr_syn_0001", "--preview", "--accept-possible-duplicate"],
        vec![
            "thr_syn_0001",
            "--preview",
            "--check",
            "--op-id",
            "op-m16-usage",
        ],
        vec!["thr_syn_0001", "--preview=yes"],
    ] {
        let (code, out, err) = fake.cli(&args, "Synthetic from stdin");
        assert_eq!(code, EXIT_USAGE, "{args:?}: {out}{err}");
        assert!(out.is_empty(), "{out}");
        assert!(err.starts_with("openwiz: "), "{err}");
    }
    assert!(fake.invocations().is_empty(), "a usage error read nothing");
    assert!(!fake.state.exists());
}

#[test]
fn the_release_build_previews_through_the_fake() {
    let fake = SendFake::new("m16_release", "send");
    let out = fake.run_process(&["send", "thr_syn_0002", "--preview", "--json"], "");
    let (code, json) = json_of(&out);
    assert_eq!(code, Some(0), "{json}");
    assert!(validate_def("preview", &json).is_empty(), "{json:#}");
    assert_eq!(
        json["effects"]["effect"]["known"]["kind"],
        "queues_behind_turn"
    );
    assert!(out.stderr.is_empty(), "no op id is made for a preview");
    assert_eq!(fake.tell_count(), 0);
    assert!(!fake.state.exists());
}

/// `send` can't work out its effects any other way than through the
/// preview's read: `work_out` is called only in `preview.rs`.
#[test]
fn only_the_preview_works_out_effects() {
    let dir = Path::new(ROOT).join("crates");
    let mut callers = Vec::new();
    let mut stack = vec![dir];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = fs::read_to_string(&path).unwrap();
                if text.contains("work_out(") && !path.ends_with("messaging/effects.rs") {
                    callers.push(path);
                }
            }
        }
    }
    assert_eq!(callers.len(), 1, "{callers:?}");
    assert!(
        callers[0].ends_with("core/src/messaging/preview.rs"),
        "{callers:?}"
    );
    let send =
        fs::read_to_string(Path::new(ROOT).join("crates/core/src/messaging/send.rs")).unwrap();
    assert!(
        send.contains("look("),
        "send does not read through the preview's look"
    );
}

/// A clock whose monotonic time jumps past every budget once the target's
/// last read returns, as if that read ran out the time.
struct JumpClock {
    base: std::time::Instant,
    jumped: AtomicBool,
}

impl Clock for JumpClock {
    fn now(&self) -> Timestamp {
        T0
    }

    fn monotonic(&self) -> std::time::Instant {
        let late = if self.jumped.load(Ordering::SeqCst) {
            60
        } else {
            0
        };
        self.base + std::time::Duration::from_secs(late)
    }
}

/// The adapter, but the target's last auxiliary read (the machine list)
/// times out: it returns what it had, with the host unknown, and the time
/// is gone.
struct LastReadTimesOut<'a> {
    inner: CloudRoomSource,
    clock: &'a JumpClock,
}

impl Messenger for LastReadTimesOut<'_> {
    fn name(&self) -> &str {
        Messenger::name(&self.inner)
    }

    fn tested_version(&self) -> &str {
        Messenger::tested_version(&self.inner)
    }

    fn check_target_id(&self, id: &SessionId) -> Result<(), InvalidRequest> {
        self.inner.check_target_id(id)
    }

    fn version(&self, deadline: std::time::Instant) -> Result<String, SourceError> {
        Messenger::version(&self.inner, deadline)
    }

    fn read_target(
        &self,
        id: &SessionId,
        deadline: std::time::Instant,
    ) -> Result<TargetRecord, SourceError> {
        let mut target = self.inner.read_target(id, deadline)?;
        target.host_connected = Some(Fact::Unknown("machine list timed out".to_owned()));
        target.problems.push(SendGap {
            subject: "machine list".to_owned(),
            detail: "timed out; not retried".to_owned(),
        });
        self.clock.jumped.store(true, Ordering::SeqCst);
        Ok(target)
    }

    fn tell(
        &self,
        id: &SessionId,
        mode: Mode,
        text: &str,
        deadline: std::time::Instant,
    ) -> TellReply {
        self.inner.tell(id, mode, text, deadline)
    }
}

#[test]
fn a_preview_after_the_budget_ran_out_is_refused_like_the_send() {
    let fake = SendFake::new("m16_budget", "send");
    let clock = JumpClock {
        base: std::time::Instant::now(),
        jumped: AtomicBool::new(false),
    };
    let messenger = LastReadTimesOut {
        inner: fake.source(),
        clock: &clock,
    };
    // thr_syn_0002 is local and running: with the host unknown, the effect
    // is unknown and no acknowledgement is needed, so only the budget
    // stands between it and the call.
    let shown = preview(&messenger, &clock, &sid("thr_syn_0002"), Mode::Queue).unwrap();
    clock.jumped.store(false, Ordering::SeqCst);
    let sent = send::send(
        &messenger,
        &fake.store(),
        &clock,
        &request("thr_syn_0002", Mode::Queue, "Synthetic", "op-m16-budget"),
    )
    .unwrap();
    assert_eq!(
        failed(&sent.result),
        Some(FailReason::BudgetExhausted),
        "{sent:#?}"
    );
    assert_eq!(shown.status, PreviewStatus::Refused, "{shown:#?}");
    assert_eq!(
        shown.refusal.as_ref().map(|r| r.reason),
        Some(FailReason::BudgetExhausted)
    );
    assert!(shown.effects.is_some(), "the effects are kept");
    assert_eq!(shown.effects, sent.effects);
    assert!(fake.tells().is_empty());

    // The CLI: exit 5, and schema-valid.
    clock.jumped.store(false, Ordering::SeqCst);
    let no_store = || -> Result<Box<dyn SendStore>, StoreError> { panic!("opened the record") };
    let no_read = || -> Result<Option<Box<dyn SendStore>>, StoreError> { panic!("opened it") };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let args: Vec<String> = ["thr_syn_0002", "--preview", "--json"]
        .iter()
        .map(|a| (*a).to_owned())
        .collect();
    let code = run_send(
        &args,
        SendContext {
            messenger: &messenger,
            open_store: &no_store,
            open_store_read_only: &no_read,
            clock: &clock,
            stdin: &mut &b""[..],
            stdin_is_terminal: false,
            new_op_id: &random_op_id,
        },
        &mut out,
        &mut err,
    );
    let json: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(code, EXIT_SEND_FAILED, "{json:#}");
    assert!(validate_def("preview", &json).is_empty(), "{json:#}");
    assert_eq!(json["refusal"]["reason"], "budget_exhausted");
}
