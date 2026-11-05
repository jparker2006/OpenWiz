//! M7 Op-id binding: the same op id with the same target, mode and text
//! makes one call. The same op id with any of them different is refused as
//! conflicting reuse, with zero calls.

use openwiz_core::messaging::send::{SendResult, Status};
use openwiz_core::messaging::{Binding, BindingField, Mode, sha256_hex};

use crate::support::*;

#[test]
fn the_same_op_id_and_binding_makes_one_call() {
    let fake = SendFake::new("m07_same", "send");
    let first = fake.report(&request("thr_syn_0001", Mode::Queue, "Synthetic", "op-m07"));
    let second = fake.report(&request("thr_syn_0001", Mode::Queue, "Synthetic", "op-m07"));
    assert_eq!(first.status, Status::Accepted);
    assert_eq!(second.status, Status::Accepted);
    assert!(matches!(
        first.result,
        SendResult::Outcome {
            replayed: false,
            ..
        }
    ));
    assert!(matches!(
        second.result,
        SendResult::Outcome { replayed: true, .. }
    ));
    match (&first.result, &second.result) {
        (SendResult::Outcome { delivery: a, .. }, SendResult::Outcome { delivery: b, .. }) => {
            assert_eq!(a, b, "the recorded outcome")
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(fake.tell_count(), 1);
    let (code, out, err) = fake.cli(
        &["thr_syn_0001", "--op-id", "op-m07", "--", "Synthetic"],
        "",
    );
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("replayed"), "{out}");
    assert_eq!(fake.tell_count(), 1);
    assert_eq!(
        fake.invocations().len(),
        4,
        "no read either: version, show, list, tell"
    );
}

#[test]
fn reuse_with_a_different_target_mode_or_text_is_refused_with_no_call() {
    let fake = SendFake::new("m07_conflict", "send");
    let _ = fake.report(&request(
        "thr_syn_0001",
        Mode::Queue,
        "Synthetic",
        "op-m07-c",
    ));
    let calls = fake.invocations().len();
    for (target, mode, text, differs) in [
        (
            "thr_syn_0006",
            Mode::Queue,
            "Synthetic",
            vec![BindingField::Target],
        ),
        (
            "thr_syn_0001",
            Mode::Steer,
            "Synthetic",
            vec![BindingField::Mode],
        ),
        (
            "thr_syn_0001",
            Mode::Queue,
            "Synthetic!",
            vec![BindingField::Text],
        ),
        (
            "thr_syn_0006",
            Mode::Steer,
            "Synthetic!",
            vec![BindingField::Target, BindingField::Mode, BindingField::Text],
        ),
    ] {
        let report = fake.report(&request(target, mode, text, "op-m07-c"));
        assert_eq!(report.status, Status::ConflictingReuse, "{report:#?}");
        match &report.result {
            SendResult::ConflictingReuse { differs: got } => assert_eq!(*got, differs),
            other => panic!("{other:?}"),
        }
    }
    let (code, json) = fake.cli_json(
        &[
            "thr_syn_0001",
            "--op-id",
            "op-m07-c",
            "--steer",
            "--",
            "Synthetic",
        ],
        "",
    );
    assert_eq!(code, 8);
    assert_eq!(json["result"]["differs"], serde_json::json!(["mode"]));
    assert_eq!(fake.invocations().len(), calls, "no read and no call");
    assert_eq!(fake.tell_count(), 1);
}

#[test]
fn a_recorded_failure_is_returned_not_retried() {
    let fake = SendFake::new("m07_failure", "send");
    let first = fake.report(&request(
        "thr_syn_9999",
        Mode::Queue,
        "Synthetic",
        "op-m07-f",
    ));
    assert_eq!(first.status, Status::Failed);
    let calls = fake.invocations().len();
    let again = fake.report(&request(
        "thr_syn_9999",
        Mode::Queue,
        "Synthetic",
        "op-m07-f",
    ));
    assert_eq!(again.status, Status::Failed);
    assert!(matches!(
        again.result,
        SendResult::Outcome { replayed: true, .. }
    ));
    assert_eq!(fake.invocations().len(), calls);
}

#[test]
fn the_binding_is_the_target_the_mode_and_a_sha256_of_the_text() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
    assert_eq!(
        sha256_hex(&[b'a'; 1_000_000]),
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
    );
    let fake = SendFake::new("m07_binding", "send");
    let text = "Synthetic ünïcödé\n";
    let report = fake.report(&request("thr_syn_0001", Mode::Steer, text, "op-m07-b"));
    assert_eq!(report.text_sha256, sha256_hex(text.as_bytes()));
    assert_eq!(report.text_bytes, text.len() as u64);
    let record = fake.record("op-m07-b");
    assert_eq!(record["binding"]["target"], "thr_syn_0001");
    assert_eq!(record["binding"]["mode"], "steer");
    assert_eq!(
        record["binding"]["text_sha256"],
        sha256_hex(text.as_bytes())
    );
    let a = Binding::new(sid("thr_syn_0001"), Mode::Queue, "x");
    assert_eq!(a.differences(&a.clone()), vec![]);
}
