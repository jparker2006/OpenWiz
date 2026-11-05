//! M1 Exact invocation: one `thread tell` per send, with an explicit
//! `--mode` and `--json`, never `auto`, a default or any excluded flag. The
//! text is one argument, byte for byte, including a leading `-`, line breaks
//! and Unicode. Invalid text is refused with no call.

use openwiz_core::messaging::send::{SendError, Status};
use openwiz_core::messaging::{MAX_TEXT_BYTES, Mode};

use crate::support::*;

/// The flags of `thread tell` OpenWiz never passes, and the mode it never
/// asks for.
pub const EXCLUDED: &[&str] = &[
    "--model",
    "--reasoning-level",
    "--service-tier",
    "--permission-mode",
    "--send-at",
    "--plan",
    "--file",
    "--image",
    "--hard",
    "auto",
];

/// The exact argument list OpenWiz passes for one send.
pub fn tell_argv(target: &str, mode: &str, text: &str) -> Vec<String> {
    [
        "thread", "tell", target, "--mode", mode, "--json", "--", text,
    ]
    .iter()
    .map(|w| (*w).to_owned())
    .collect()
}

#[test]
fn one_send_makes_one_tell_in_the_chosen_shape() {
    let fake = SendFake::new("m01_one_tell", "send");
    let report = fake.report(&request(
        "thr_syn_0001",
        Mode::Queue,
        "Synthetic hello",
        "op-m01-one",
    ));
    assert_eq!(report.status, Status::Accepted, "{report:#?}");
    assert_eq!(
        fake.tells(),
        vec![tell_argv("thr_syn_0001", "queue", "Synthetic hello")]
    );
    assert_eq!(fake.tell_count(), 1);
    // The reads before it: the version, the target, and the list row the
    // effects need. Nothing else.
    let calls = fake.invocations();
    assert_eq!(
        calls[..calls.len() - 1],
        [
            vec!["--version".to_owned()],
            ["thread", "show", "thr_syn_0001", "--json"]
                .map(str::to_owned)
                .to_vec(),
            ["thread", "list", "--json"].map(str::to_owned).to_vec(),
        ]
    );
    for word in EXCLUDED {
        assert!(
            !calls.iter().flatten().any(|w| w == word),
            "{word} was passed: {calls:?}"
        );
    }
}

#[test]
fn the_text_is_one_argument_byte_for_byte() {
    let fake = SendFake::new("m01_byte_for_byte", "send");
    let exact_cap = "a".repeat(MAX_TEXT_BYTES);
    let texts = [
        "-starts with a dash",
        "--json",
        "--mode=auto",
        "--file /synthetic/notes.txt",
        "-- after the end of options",
        "line one\nline two\r\n\nthree\n",
        "tab\tquotes \" ' ` $HOME ; | & > < * ? ~ \\ end",
        "ünïcödé · 中文 · 🙂 · \u{202E}bidi\u{202C} · \u{200B}zero width",
        "  leading and trailing spaces  ",
        exact_cap.as_str(),
    ];
    for (i, text) in texts.iter().enumerate() {
        let report = fake.report(&request(
            "thr_syn_0001",
            Mode::Queue,
            text,
            &format!("op-m01-text-{i}"),
        ));
        assert_eq!(report.status, Status::Accepted, "{text:?}: {report:#?}");
        let tells = fake.tells();
        assert_eq!(tells.len(), i + 1, "one tell per send");
        assert_eq!(
            tells[i],
            tell_argv("thr_syn_0001", "queue", text),
            "{text:?}"
        );
    }
    assert_eq!(fake.tell_count(), texts.len() as u64);
}

#[test]
fn invalid_text_is_refused_with_no_call() {
    assert_eq!(MAX_TEXT_BYTES, 16 * 1024);
    let fake = SendFake::new("m01_invalid_text", "send");
    let over = "a".repeat(MAX_TEXT_BYTES + 1);
    // 16 383 ASCII bytes and one 2-byte character: 16 385 bytes, 16 384
    // characters. The cap counts bytes.
    let over_by_bytes = format!("{}é", "a".repeat(MAX_TEXT_BYTES - 1));
    assert_eq!(over_by_bytes.chars().count(), MAX_TEXT_BYTES);
    for (i, text) in ["", " \n\t ", "a\0b", over.as_str(), over_by_bytes.as_str()]
        .iter()
        .enumerate()
    {
        let op_id = format!("op-m01-invalid-{i}");
        match fake.send(&request("thr_syn_0001", Mode::Queue, text, &op_id)) {
            Err(SendError::Invalid(_)) => {}
            other => panic!("{text:?} was not refused: {other:?}"),
        }
        assert!(
            !fake.record_path(&op_id).exists(),
            "{text:?}: nothing is claimed"
        );
    }
    assert_eq!(
        fake.invocations(),
        Vec::<Vec<String>>::new(),
        "no call at all"
    );
}

#[test]
fn the_cli_passes_dash_leading_and_stdin_text_byte_for_byte() {
    let fake = SendFake::new("m01_cli_text", "send");
    let (code, _, err) = fake.cli(
        &[
            "thr_syn_0001",
            "--op-id",
            "op-m01-cli-1",
            "--",
            "-x --steer",
        ],
        "",
    );
    assert_eq!(code, 0, "{err}");
    let piped = "from stdin\nwith a final newline\n";
    let (code, _, err) = fake.cli(&["thr_syn_0001", "--op-id", "op-m01-cli-2"], piped);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        fake.tells(),
        vec![
            tell_argv("thr_syn_0001", "queue", "-x --steer"),
            tell_argv("thr_syn_0001", "queue", piped),
        ]
    );
}

#[test]
fn the_cli_refuses_invalid_text_with_no_call() {
    let fake = SendFake::new("m01_cli_invalid", "send");
    let over = "a".repeat(MAX_TEXT_BYTES + 1);
    for (args, stdin) in [
        (vec!["thr_syn_0001"], ""),
        (vec!["thr_syn_0001"], "   \n"),
        (vec!["thr_syn_0001", "--", "a\0b"], ""),
        (vec!["thr_syn_0001", "--", over.as_str()], ""),
        (vec!["thr_syn_0001"], over.as_str()),
    ] {
        let (code, out, err) = fake.cli(&args, stdin);
        assert_eq!(code, 2, "{args:?}: {err}");
        assert!(out.is_empty(), "{out}");
        assert!(err.starts_with("openwiz: "), "{err}");
    }
    assert_eq!(
        fake.invocations(),
        Vec::<Vec<String>>::new(),
        "no call at all"
    );
}

#[test]
fn invalid_op_ids_are_refused_with_no_call() {
    let fake = SendFake::new("m01_invalid_op_ids", "send");
    let long = "a".repeat(65);
    for bad in [
        "",
        "-x",
        ".x",
        "_x",
        "a/b",
        "a b",
        "a.b",
        "ünï",
        long.as_str(),
    ] {
        assert!(
            openwiz_core::messaging::OpId::parse(bad).is_err(),
            "accepted {bad:?}"
        );
        let (code, _, err) = fake.cli(&["thr_syn_0001", "--op-id", bad, "--", "Synthetic"], "");
        assert_eq!(code, 2, "{bad:?}: {err}");
    }
    for good in ["a", "op-1", "A_b-9", &"a".repeat(64)] {
        assert!(openwiz_core::messaging::OpId::parse(good).is_ok(), "{good}");
    }
    assert_eq!(fake.invocations(), Vec::<Vec<String>>::new());
}
