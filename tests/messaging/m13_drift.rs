use openwiz_core::messaging::Mode;
use openwiz_core::messaging::send::{Delivery, Status, UncertainReason};

use serde_json::{Value, json};

use crate::m04_outcome_mapping::{delivery, uncertain_reason};
use crate::support::*;

#[test]
fn an_unexpected_receipt_is_uncertain_with_a_gap() {
    for (file, reason) in [
        (
            "tell-drift-extra-field.json",
            UncertainReason::UnexpectedReceipt,
        ),
        (
            "tell-drift-unknown-delivery.json",
            UncertainReason::UnexpectedReceipt,
        ),
        (
            "tell-drift-unknown-waiting-kind.json",
            UncertainReason::UnexpectedReceipt,
        ),
        (
            "tell-drift-no-queued-message.json",
            UncertainReason::UnexpectedReceipt,
        ),
        (
            "tell-drift-unsafe-queued-id.json",
            UncertainReason::UnexpectedReceipt,
        ),
        (
            "tell-drift-delivery-not-string.json",
            UncertainReason::UnexpectedReceipt,
        ),
        ("tell-drift-array.json", UncertainReason::UnexpectedReceipt),
        ("tell-not-json.txt", UncertainReason::NotJson),
    ] {
        let fake = SendFake::with(
            &format!("m13_{file}"),
            vec![tell_response("thr_syn_0001", "queue", receipt(file))],
        );
        let report = fake.report(&request("thr_syn_0001", Mode::Queue, "Synthetic", "op-m13"));
        assert_eq!(report.status, Status::Uncertain, "{file}: {report:#?}");
        assert_eq!(uncertain_reason(&report), reason, "{file}");
        assert!(
            report.gaps.iter().any(|g| g.subject == "thread tell"),
            "{file}: a gap names the receipt: {:#?}",
            report.gaps
        );
        assert!(!matches!(delivery(&report), Delivery::Accepted { .. }));
        let record = fake.record("op-m13");
        assert_eq!(record["delivery"]["kind"], "uncertain", "{file}");
        assert!(!record["gaps"].as_array().unwrap().is_empty(), "{file}");
    }
}

/// A `send` fake whose `room-cli --version` gives `answer` (an inline
/// stdout, or an exit with stderr); every other answer is the `send`
/// scenario's.
fn with_version(test: &str, answer: Value) -> SendFake {
    let mut response = json!({"argv": ["--version"]});
    for (k, v) in answer.as_object().expect("an object") {
        response[k] = v.clone();
    }
    SendFake::with(test, vec![response])
}

/// The report's `source`, as JSON.
fn source_of(report: &openwiz_core::messaging::send::SendReport) -> Value {
    serde_json::to_value(report).expect("report JSON")["source"].clone()
}

#[test]
fn an_untested_version_never_stops_a_send() {
    let tested = SendFake::new("m13_version_tested", "send");
    let wanted = tested.report(&request(
        "thr_syn_0001",
        Mode::Queue,
        "Synthetic",
        "op-m13-v",
    ));
    assert_eq!(
        source_of(&wanted),
        json!({"name": "room-cli", "version": {"known": "v127"}, "tested_version": "v127", "version_tested": true}),
        "the tested version is reported too"
    );
    for (i, version) in ["0.44.0", "0.43.1"].iter().enumerate() {
        let fake = with_version(
            &format!("m13_version_{i}"),
            json!({"stdout": format!("{version}\n")}),
        );
        let report = fake.report(&request(
            "thr_syn_0001",
            Mode::Queue,
            "Synthetic",
            "op-m13-v",
        ));
        assert_eq!(
            delivery(&report),
            delivery(&wanted),
            "{version}: sent as on v127"
        );
        assert_eq!(fake.tell_count(), 1, "{version}: one call");
        assert_eq!(
            source_of(&report),
            json!({"name": "room-cli", "version": {"known": version}, "tested_version": "v127", "version_tested": false}),
            "{version}"
        );
        assert_eq!(
            fake.record("op-m13-v")["source"],
            source_of(&report),
            "{version}: recorded"
        );
        assert!(
            !report
                .gaps
                .iter()
                .any(|g| g.subject == "room-cli --version"),
            "{version}: a known version is not a gap: {:#?}",
            report.gaps
        );

        let (code, out, err) = fake.cli(&["thr_syn_0002", "--", "Synthetic two"], "");
        assert_eq!(code, 0, "{version}: {err}");
        assert!(
            out.contains(&format!("room-cli {version} (untested; live-tested: v127)")),
            "{version}: the text names the version as the reads do:\n{out}"
        );
    }
}

#[test]
fn an_unrecognised_or_unreadable_version_never_stops_a_send() {
    for (i, (answer, why)) in [
        (
            json!({"stdout": "Synthetic not a version\n"}),
            "printed a version that is not N.N.N or v<N> (detail withheld)",
        ),
        (
            json!({"exit": 1, "stderr": "Synthetic failure SYNTHETIC_TOKEN=synthetic-secret-value"}),
            "exited 1",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let fake = with_version(&format!("m13_version_unknown_{i}"), answer);
        let report = fake.report(&request("thr_syn_0001", Mode::Queue, "Synthetic", "op-m13-u"));
        assert_ne!(report.status, Status::Failed, "{why}: {report:#?}");
        assert_eq!(fake.tell_count(), 1, "{why}: one call");
        let source = source_of(&report);
        let reason = source["version"]["unknown"].as_str().unwrap_or_default();
        assert!(
            reason.starts_with("room-cli --version ") && reason.contains(why),
            "{why}: {source}"
        );
        assert_eq!(source["version_tested"], false);
        assert!(
            report.gaps.iter().any(|g| g.subject == "room-cli --version" && g.detail == reason),
            "{why}: a gap: {:#?}",
            report.gaps
        );
        let (_, out, err) = fake.cli(&["thr_syn_0002", "--", "Synthetic two"], "");
        let json = serde_json::to_string(&report).unwrap();
        for text in [json.as_str(), out.as_str(), err.as_str()] {
            assert!(
                !text.contains("Synthetic not a version") && !text.contains("synthetic-secret-value"),
                "{why}: the provider's text is shown: {text}"
            );
        }
    }
}

#[test]
fn a_failed_version_read_never_echoes_its_error_text_in_send() {
    const MARKER: &str = "Synthetic private version marker";
    let fake = with_version(
        "m13_version_failed",
        json!({"exit": 1, "stderr": format!("Error: HTTP 500: {MARKER}")}),
    );
    let report = fake.report(&request(
        "thr_syn_0001",
        Mode::Queue,
        "Synthetic",
        "op-m13-f",
    ));
    assert_eq!(
        source_of(&report)["version"]["unknown"],
        "room-cli --version exited 1 (detail withheld)"
    );
    let (_, out, err) = fake.cli(
        &[
            "thr_syn_0002",
            "--op-id",
            "op-m13-f2",
            "--",
            "Synthetic two",
        ],
        "",
    );
    let (_, json) = fake.cli_json(
        &[
            "thr_syn_0003",
            "--op-id",
            "op-m13-f3",
            "--",
            "Synthetic three",
        ],
        "",
    );
    for (what, text) in [
        ("report", serde_json::to_string(&report).unwrap()),
        ("text", format!("{out}{err}")),
        ("JSON", json.to_string()),
        ("record", fake.record("op-m13-f").to_string()),
        ("record 2", fake.record("op-m13-f2").to_string()),
        ("record 3", fake.record("op-m13-f3").to_string()),
    ] {
        assert!(!text.contains(MARKER), "{what}: {text}");
        assert!(
            text.contains("exited 1 (detail withheld)"),
            "{what}: {text}"
        );
    }
}

#[test]
fn an_earlier_untested_version_record_replays_as_failed() {
    let fake = SendFake::new("m13_version_legacy", "send");
    let args = ["thr_syn_9999", "--op-id", "op-m13-old", "--", "Synthetic"];
    let (code, _, err) = fake.cli(&args, "");
    assert_eq!(code, 3, "the planted record starts as not found: {err}");
    let mut record = fake.record("op-m13-old");
    record["delivery"]["reason"] = json!("untested_version");
    record["delivery"]["text"] =
        json!("failed: room-cli reports 0.44.0, not the tested 0.43.1; nothing was sent");
    record.as_object_mut().unwrap().remove("source");
    std::fs::write(fake.record_path("op-m13-old"), record.to_string()).unwrap();

    let (code, out, err) = fake.cli(&args, "");
    assert_eq!(code, 5, "{out}{err}");
    assert!(out.contains("replayed"), "{out}");
    assert_eq!(fake.tell_count(), 0);
}
