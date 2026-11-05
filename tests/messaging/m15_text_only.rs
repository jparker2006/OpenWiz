//! M15 Text only: a file path in the text is sent as text, never uploaded
//! or read.
//!
//! **Updated for B2 (the implementation)**, with this recorded reason: sheet the check
//!  adds attachments, through `--attach
//! <path>` (staged first) and `--attachment <id>` only, and `--file` and
//! `--image` only with staged absolute paths, built by the allowlist. So
//! `openwiz send` still has no `--file` or `--image` option, and a path in
//! the text is still sent as text: "a path in the text is still sent as
//! text" . AD1 to AD13 prove the attachments.

use std::fs;

use openwiz_core::messaging::Mode;
use openwiz_core::messaging::send::Status;

use crate::m01_exact_invocation::tell_argv;
use crate::support::*;

#[test]
fn there_is_no_file_or_image_option() {
    let fake = SendFake::new("m15_no_option", "send");
    for args in [
        vec![
            "thr_syn_0001",
            "--file",
            "/synthetic/x.txt",
            "--",
            "Synthetic",
        ],
        vec![
            "thr_syn_0001",
            "--image",
            "/synthetic/x.png",
            "--",
            "Synthetic",
        ],
        vec!["thr_syn_0001", "--file=/synthetic/x.txt", "--", "Synthetic"],
        vec![
            "thr_syn_0001",
            "--image=/synthetic/x.png",
            "--",
            "Synthetic",
        ],
        // `--attach` needs a value, and `--attachment` an attachment id.
        vec!["thr_syn_0001", "--attach"],
        vec![
            "thr_syn_0001",
            "--attachment",
            "/synthetic/x.txt",
            "--",
            "S",
        ],
    ] {
        let (code, out, err) = fake.cli(&args, "");
        assert_eq!(code, 2, "{args:?}: {out}{err}");
    }
    assert!(fake.invocations().is_empty());
}

#[test]
fn a_file_path_in_the_text_is_sent_as_text() {
    let fake = SendFake::new("m15_path_as_text", "send");
    // A real file: its contents must never be read or sent.
    let file = fake.dir.join("synthetic-attachment.txt");
    fs::write(&file, "Synthetic file contents that must never be sent").unwrap();
    let path = file.to_str().unwrap().to_owned();
    let text = format!("please look at {path}");
    for (i, text) in [path.as_str(), text.as_str()].into_iter().enumerate() {
        let report = fake.report(&request(
            "thr_syn_0001",
            Mode::Queue,
            text,
            &format!("op-m15-{i}"),
        ));
        assert_eq!(report.status, Status::Accepted);
    }
    let tells = fake.tells();
    assert_eq!(
        tells,
        vec![
            tell_argv("thr_syn_0001", "queue", &path),
            tell_argv("thr_syn_0001", "queue", &text),
        ]
    );
    for tell in &tells {
        assert!(!tell.iter().any(|a| a.contains("must never be sent")));
        assert!(!tell.iter().any(|a| a == "--file" || a == "--image"));
    }
    let (code, _, err) = fake.cli(&["thr_syn_0006", "--", &path], "");
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        fake.tells().last().unwrap(),
        &tell_argv("thr_syn_0006", "queue", &path)
    );
}
