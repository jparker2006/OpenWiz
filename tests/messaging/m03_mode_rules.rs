//! M3 Mode rules: the requested mode is the one passed: queue by default,
//! steer only when asked for, never `auto`. An unsupported mode is reported
//! as such, with no second call and no other mode tried.

use openwiz_core::messaging::Mode;
use openwiz_core::messaging::send::{Delivery, FailReason, SendResult, Status};

use crate::m01_exact_invocation::tell_argv;
use crate::support::*;

#[test]
fn the_requested_mode_is_the_one_passed() {
    let fake = SendFake::new("m03_requested_mode", "send");
    let (code, queue) = fake.cli_json(&["thr_syn_0006", "--", "Synthetic one"], "");
    assert_eq!(code, 0, "{queue}");
    let (code, steer) = fake.cli_json(&["thr_syn_0006", "--steer", "--", "Synthetic two"], "");
    assert_eq!(code, 0);
    assert_eq!(queue["requested_mode"], "queue");
    assert_eq!(steer["requested_mode"], "steer");
    assert_eq!(
        fake.tells(),
        vec![
            tell_argv("thr_syn_0006", "queue", "Synthetic one"),
            tell_argv("thr_syn_0006", "steer", "Synthetic two"),
        ]
    );
}

#[test]
fn an_unsupported_mode_is_reported_with_no_second_call() {
    let fake = SendFake::with(
        "m03_unsupported",
        vec![tell_response(
            "thr_syn_0002",
            "steer",
            serde_json::json!({"exit":1, "stderr":"Error: HTTP 400: Unsupported mode"}),
        )],
    );
    let report = fake.report(&request(
        "thr_syn_0002",
        Mode::Steer,
        "Synthetic steer",
        "op-m03-unsupported",
    ));
    assert_eq!(report.status, Status::Failed);
    assert_eq!(report.requested_mode, Mode::Steer);
    match &report.result {
        SendResult::Outcome {
            delivery: Delivery::Failed { reason, text },
            ..
        } => {
            assert_eq!(*reason, FailReason::UnsupportedMode);
            assert!(text.contains("unsupported"), "{text}");
        }
        other => panic!("{other:?}"),
    }
    assert!(
        report.summary.starts_with("Steer requested · failed"),
        "{}",
        report.summary
    );
    assert_eq!(
        fake.tells(),
        vec![tell_argv("thr_syn_0002", "steer", "Synthetic steer")],
        "one call, in the requested mode only"
    );
    let (code, _, _) = fake.cli(
        &["thr_syn_0002", "--steer", "--", "Synthetic steer again"],
        "",
    );
    assert_eq!(code, 5);
    assert_eq!(fake.tell_count(), 2, "one call per send, never a fallback");
    assert!(fake.tells().iter().all(|t| t[4] == "steer"));
}

#[test]
fn the_cli_has_no_other_mode() {
    let fake = SendFake::new("m03_no_other_mode", "send");
    for args in [
        vec!["thr_syn_0001", "--mode", "auto", "--", "Synthetic"],
        vec!["thr_syn_0001", "--mode=queue", "--", "Synthetic"],
        vec!["thr_syn_0001", "--auto", "--", "Synthetic"],
        vec!["thr_syn_0001", "--steer=no", "--", "Synthetic"],
    ] {
        let (code, _, err) = fake.cli(&args, "");
        assert_eq!(code, 2, "{args:?}: {err}");
    }
    assert!(fake.invocations().is_empty());
}
