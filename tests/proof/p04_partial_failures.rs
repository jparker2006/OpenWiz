//! P4 Partial failures: errors, timeouts, malformed responses and a missing
//! session give an honest partial result or an explicit error, never
//! invented data.

use std::process::Command;
use std::time::{Duration, Instant};

use openwiz_core::envelope::{Completeness, GapKind};
use openwiz_core::model::{Fact, Grouping, SessionId};
use openwiz_core::ops::{ListRequest, OpError};
use openwiz_core::source::SourceError;

use crate::support::*;

/// Slack on top of a budget for process start-up and clean-up.
const SLACK: Duration = Duration::from_secs(3);

fn still_running(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid.trim()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[test]
fn thread_list_error_is_an_explicit_error() {
    let fake = Fake::new("p04_thread_list_error", "thread-list-error");
    match fake.list(&ListRequest::default()) {
        Err(OpError::ListFailed(SourceError::CommandFailed { exit_code, detail })) => {
            assert_eq!(exit_code, Some(1));
            assert!(
                detail.contains("HTTP 500"),
                "keeps the source's error line: {detail}"
            );
        }
        other => panic!("expected an explicit list error, got {other:?}"),
    }
}

#[test]
fn thread_list_timeout_is_explicit_bounded_and_cleaned_up() {
    let fake = Fake::new("p04_thread_list_timeout", "thread-list-timeout");
    let budget = Duration::from_secs(2);
    let started = Instant::now();
    let result = fake.list(&ListRequest {
        budget,
        ..ListRequest::default()
    });
    assert!(
        started.elapsed() < budget + SLACK,
        "took {:?}",
        started.elapsed()
    );
    assert!(
        matches!(
            result,
            Err(OpError::ListFailed(SourceError::TimedOut { .. }))
        ),
        "expected an explicit timeout, got {result:?}"
    );
    let pid = fake.hung_pid().expect("the fake hung");
    assert!(
        !still_running(&pid),
        "the hung room-cli is still running or was not reaped"
    );
}

#[test]
fn malformed_thread_list_is_an_explicit_error() {
    for scenario in ["thread-list-not-json", "thread-list-truncated"] {
        let fake = Fake::new(&format!("p04_malformed_{scenario}"), scenario);
        let result = fake.list(&ListRequest::default());
        assert!(
            matches!(
                result,
                Err(OpError::ListFailed(SourceError::Malformed { .. }))
            ),
            "{scenario}: expected an explicit malformed error, got {result:?}"
        );
    }
}

#[test]
fn project_list_failure_gives_a_partial_ungrouped_result() {
    let fake = Fake::new("p04_project_list_failure", "project-list-error");
    let env = fake
        .list(&ListRequest::default())
        .expect("honest partial list");
    assert_eq!(env.coverage.state, Completeness::Partial);
    assert!(
        has_gap(&env.gaps, GapKind::CommandFailed, "project list"),
        "{:?}",
        env.gaps
    );
    assert_eq!(env.data.sessions.len(), 6, "sessions are still listed");
    for s in &env.data.sessions {
        assert!(
            matches!(&s.group, Grouping::Ungrouped { reason } if !reason.is_empty()),
            "{}: grouping cannot be verified without the project list, got {:?}",
            s.id.0,
            s.group
        );
    }
}

#[test]
fn detail_read_error_gives_a_partial_result_with_unknown_configuration() {
    let fake = Fake::new("p04_detail_read_error", "show-error");
    let env = fake.list(&long()).expect("honest partial list");
    assert_eq!(env.coverage.state, Completeness::Partial);
    assert!(
        has_gap(&env.gaps, GapKind::CommandFailed, "thr_syn_0002"),
        "{:?}",
        env.gaps
    );
    let failed = &session(&env, "thr_syn_0002").reported_configuration;
    unknown_reason(&failed.model);
    unknown_reason(&failed.effort);
    let fine = &session(&env, "thr_syn_0001").reported_configuration;
    assert_eq!(fine.model, Fact::Known("synthetic-model-a".to_owned()));
}

#[test]
fn detail_read_timeout_gives_a_partial_result_within_the_budget() {
    let fake = Fake::new("p04_detail_read_timeout", "show-timeout");
    let budget = Duration::from_secs(3);
    let started = Instant::now();
    let env = fake
        .list(&ListRequest { budget, ..long() })
        .expect("honest partial list");
    assert!(
        started.elapsed() < budget + SLACK,
        "took {:?}",
        started.elapsed()
    );
    assert_eq!(env.coverage.state, Completeness::Partial);
    assert!(
        has_gap(&env.gaps, GapKind::TimedOut, "thr_syn_0002")
            || has_gap(&env.gaps, GapKind::BudgetExhausted, "thr_syn_0002"),
        "{:?}",
        env.gaps
    );
    unknown_reason(&session(&env, "thr_syn_0002").reported_configuration.model);
    let pid = fake.hung_pid().expect("the fake hung");
    assert!(
        !still_running(&pid),
        "the hung room-cli is still running or was not reaped"
    );
}

#[test]
fn wrong_types_become_unknowns_not_invented_values() {
    let fake = Fake::new("p04_wrong_types", "thread-list-wrong-types");
    let env = fake
        .list(&ListRequest::default())
        .expect("honest partial list");
    assert_eq!(env.coverage.state, Completeness::Partial);
    unknown_reason(&session(&env, "thr_syn_0001").status);
    unknown_reason(&session(&env, "thr_syn_0002").updated_at);
    assert!(matches!(
        session(&env, "thr_syn_0003").group,
        Grouping::Ungrouped { .. }
    ));
    unknown_reason(&session(&env, "thr_syn_0004").recap);
    unknown_reason(&session(&env, "thr_syn_0005").parent);
    unknown_reason(&session(&env, "thr_syn_0006").location);
}

/// Is `argv` a detail read (`thread show` or `thread output`) of `id`?
fn is_detail_read_of(argv: &[String], id: &str) -> bool {
    argv.len() == 4
        && argv[0] == "thread"
        && (argv[1] == "show" || argv[1] == "output")
        && argv[2] == id
}

/// The not-found detail read is the last read, and the only one of `id`.
fn assert_stops_at_not_found(label: &str, calls: &[Vec<String>], id: &str) {
    let detail_reads = calls.iter().filter(|a| is_detail_read_of(a, id)).count();
    assert_eq!(
        detail_reads, 1,
        "{label}: one detail read of {id}: {calls:?}"
    );
    assert!(
        calls.last().is_some_and(|a| is_detail_read_of(a, id)),
        "{label}: no read after the not-found detail read: {calls:?}"
    );
}

/// No further read is made after not-found, so the invocation log is
/// checked. `--json` prints nothing, or a JSON value with no `data`.
#[test]
fn show_of_a_nonexistent_session_is_not_found() {
    let fake = Fake::new("p04_show_nonexistent", "normal");
    match fake.show(MISSING_ID) {
        Err(OpError::NotFound(id)) => assert_eq!(id, SessionId(MISSING_ID.to_owned())),
        other => panic!("expected not found, got {other:?}"),
    }
    assert_stops_at_not_found("core show", &fake.invocations(), MISSING_ID);

    for args in [
        &["show", MISSING_ID][..],
        &["show", MISSING_ID, "--json"][..],
    ] {
        let fake = Fake::new(
            &format!("p04_show_nonexistent_cli_{}", args.len()),
            "normal",
        );
        let (code, out, err) = fake.cli(args);
        assert_ne!(code, 0);
        if args.contains(&"--json") && !out.trim().is_empty() {
            let value: serde_json::Value = serde_json::from_str(&out)
                .unwrap_or_else(|e| panic!("--json prints JSON or nothing ({e}): {out}"));
            assert!(
                value.get("data").is_none(),
                "no session is printed as if it existed: {out}"
            );
        } else {
            assert!(
                out.trim().is_empty(),
                "nothing is printed as if it existed: {out}"
            );
        }
        assert!(err.to_lowercase().contains("not found"), "{err}");
        assert_stops_at_not_found(&format!("{args:?}"), &fake.invocations(), MISSING_ID);
    }
}

#[test]
fn show_of_an_unsafe_id_is_refused_without_running_room_cli() {
    let fake = Fake::new("p04_show_unsafe_id", "normal");
    let result = fake.show("--work-status");
    assert!(
        matches!(result, Err(OpError::InvalidRequest(_))),
        "an unsafe id is an invalid request, not a missing session: {result:?}"
    );
    let calls = fake.invocations();
    assert!(
        calls
            .iter()
            .all(|argv| !argv.iter().any(|a| a == "--work-status")),
        "the id reached room-cli: {calls:?}"
    );
}

#[test]
fn final_output_read_error_gives_a_partial_show() {
    let fake = Fake::new("p04_final_output_read_error", "show-output-error");
    let env = fake.show("thr_syn_0002").expect("honest partial show");
    assert_eq!(env.coverage.state, Completeness::Partial);
    assert!(
        has_gap(&env.gaps, GapKind::CommandFailed, "thr_syn_0002"),
        "{:?}",
        env.gaps
    );
    unknown_reason(&env.data.final_output);
    unknown_reason(&env.data.reported_error);
    assert_eq!(
        env.data.session.status,
        Fact::Known("active".to_owned()),
        "the rest is still read"
    );
}

#[test]
fn final_output_read_timeout_gives_a_partial_show_within_the_budget() {
    let fake = Fake::new("p04_final_output_read_timeout", "show-output-timeout");
    let budget = Duration::from_secs(3);
    let started = Instant::now();
    let env = openwiz_core::ops::show(
        &fake.source(),
        &clock(),
        &openwiz_core::ops::ShowRequest {
            id: SessionId("thr_syn_0002".to_owned()),
            budget,
        },
    )
    .expect("honest partial show");
    assert!(
        started.elapsed() < budget + SLACK,
        "took {:?}",
        started.elapsed()
    );
    assert_eq!(env.coverage.state, Completeness::Partial);
    assert!(
        has_gap(&env.gaps, GapKind::TimedOut, "thr_syn_0002")
            || has_gap(&env.gaps, GapKind::BudgetExhausted, "thr_syn_0002"),
        "{:?}",
        env.gaps
    );
    unknown_reason(&env.data.final_output);
    let pid = fake.hung_pid().expect("the fake hung");
    assert!(
        !still_running(&pid),
        "the hung room-cli is still running or was not reaped"
    );
}

/// Each read has a 15-second timeout, and there
/// are no retries; a timeout becomes a gap. With the default 30-second
/// budget, a read that never answers is stopped after 15 seconds, once.
#[test]
fn a_read_that_never_answers_is_stopped_after_15_seconds_and_not_retried() {
    assert_eq!(
        openwiz_core::limits::READ_TIMEOUT,
        Duration::from_secs(15),
        "the approved read timeout"
    );
    let fifteen = Duration::from_secs(15);

    let fake = Fake::new("p04_read_timeout_thread_list", "thread-list-timeout");
    let started = Instant::now();
    let result = fake.list(&ListRequest::default());
    let took = started.elapsed();
    assert!(
        matches!(
            result,
            Err(OpError::ListFailed(SourceError::TimedOut { .. }))
        ),
        "{result:?}"
    );
    assert!(took >= fifteen && took < fifteen + SLACK, "took {took:?}");
    let lists = fake
        .invocations()
        .iter()
        .filter(|a| a.len() >= 2 && a[0] == "thread" && a[1] == "list")
        .count();
    assert_eq!(lists, 1, "no retry: {:?}", fake.invocations());
    let pid = fake.hung_pid().expect("the fake hung");
    assert!(
        !still_running(&pid),
        "the hung room-cli is still running or was not reaped"
    );

    let fake = Fake::new("p04_read_timeout_thread_show", "show-timeout");
    let started = Instant::now();
    let env = fake.list(&long()).expect("honest partial list");
    let took = started.elapsed();
    assert!(took >= fifteen && took < fifteen + SLACK, "took {took:?}");
    assert!(
        has_gap(&env.gaps, GapKind::TimedOut, "thr_syn_0002"),
        "the read timed out; the budget did not run out: {:?}",
        env.gaps
    );
    unknown_reason(&session(&env, "thr_syn_0002").reported_configuration.model);
    for id in ["thr_syn_0001", "thr_syn_0003", "thr_syn_0006"] {
        assert!(
            matches!(
                session(&env, id).reported_configuration.model,
                Fact::Known(_)
            ),
            "{id} is still enriched"
        );
    }
    let shows = fake
        .invocations()
        .iter()
        .filter(|a| is_detail_read_of(a, "thr_syn_0002"))
        .count();
    assert_eq!(shows, 1, "no retry: {:?}", fake.invocations());
    let pid = fake.hung_pid().expect("the fake hung");
    assert!(
        !still_running(&pid),
        "the hung room-cli is still running or was not reaped"
    );
}

/// The exit codes `openwiz --help` documents: 0 a result
/// (complete or partial), 1 a failed read, 2 a usage error or out-of-bounds
/// request, 3 not found. An untested `room-cli` version is never an
/// error (exit 0, marked untested). On an error stdout is
/// empty and stderr has one `openwiz: ` message; a usage error reads nothing.
#[test]
fn exit_codes_are_documented_and_errors_go_to_stderr() {
    let cases: &[(&str, &[&str], i32)] = &[
        ("normal", &["ls"], 0),
        ("normal", &["ls", "-l", "--json"], 0),
        ("normal", &["show", "thr_syn_0001"], 0),
        ("project-list-error", &["ls"], 0),
        ("show-output-error", &["show", "thr_syn_0002", "--json"], 0),
        ("thread-list-error", &["ls"], 1),
        ("thread-list-not-json", &["ls", "--json"], 1),
        ("show-error", &["show", "thr_syn_0002"], 1),
        ("normal", &[], 2),
        ("normal", &["ls", "--bogus"], 2),
        ("normal", &["ls", "--limit"], 2),
        ("normal", &["ls", "--limit", "many"], 2),
        ("normal", &["ls", "--limit", "201"], 2),
        ("normal", &["ls", "--offset", "-1"], 2),
        ("normal", &["show"], 2),
        ("normal", &["show", "thr_syn_0001", "thr_syn_0002"], 2),
        ("normal", &["show", "--work-status"], 2),
        ("normal", &["rm", "thr_syn_0001"], 2),
        ("normal", &["show", MISSING_ID], 3),
        ("normal", &["show", MISSING_ID, "--json"], 3),
        ("version-0.44.0", &["ls"], 0),
        ("version-0.44.0", &["show", "thr_syn_0001", "--json"], 0),
        ("version-0.43.1", &["ls", "-l"], 0),
        ("version-unrecognised", &["ls"], 0),
    ];
    for (i, (scenario, args, wanted)) in cases.iter().enumerate() {
        let fake = Fake::new(&format!("p04_exit_code_{i}"), scenario);
        let (code, out, err) = fake.cli(args);
        assert_eq!(code, *wanted, "{scenario} openwiz {args:?}: {err}");
        if *wanted == 0 {
            assert!(!out.is_empty(), "{scenario} {args:?} prints a result");
        } else {
            assert!(out.is_empty(), "{scenario} {args:?}: stdout: {out}");
            assert!(
                err.starts_with("openwiz: "),
                "{scenario} {args:?}: stderr: {err}"
            );
        }
        if *wanted == 2 {
            assert!(
                fake.invocations().is_empty(),
                "{args:?}: a usage error reads nothing: {:?}",
                fake.invocations()
            );
        }
    }
}

/// `openwiz --help` documents the commands, the sort key
/// and the exit codes, and reads nothing.
#[test]
fn help_documents_the_commands_the_order_and_the_exit_codes() {
    for flag in ["--help", "-h"] {
        let fake = Fake::new(&format!("p04_help_{flag}"), "normal");
        let (code, out, err) = fake.cli(&[flag]);
        assert_eq!(code, 0, "{err}");
        for wanted in [
            "openwiz ls [-l] [-a] [--hidden] [--limit N] [--offset N] [--json]",
            "openwiz show <id> [--json]",
            "Order: most recently updated first, then by session id; sessions whose update time is unknown come last.",
            "Exit codes:",
            "  0  a result was printed, complete or partial",
            "  1  a read failed",
            "  2  usage error, or a request out of bounds",
            "  3  show: the session was not found",
            "Every run reads room-cli --version and shows it. Any version runs; one other than the live-tested v127 is marked untested.",
        ] {
            assert!(
                out.contains(wanted),
                "{flag}: help should contain {wanted:?}:\n{out}"
            );
        }
        assert!(
            !out.lines().any(|l| l.starts_with("  4  ")),
            "{flag}: no exit code 4, the version is never a gate:\n{out}"
        );
        assert!(fake.invocations().is_empty(), "help reads nothing");
    }
}

/// OpenWiz 0.0.2 is read-only: there is no `send`. The real `openwiz`
/// program treats it as an unknown command, reads and writes nothing, and
/// its help never mentions it.
#[test]
fn send_is_an_unknown_command_and_help_never_mentions_it() {
    let fake = Fake::new("p04_no_send", "normal");
    for args in [&["send", "thr_syn_0001", "hello"][..], &["send"][..]] {
        let (code, out, err) = fake.program_cli(args);
        assert_eq!(code, 2, "openwiz {args:?}: {out}{err}");
        assert!(out.is_empty(), "openwiz {args:?}: stdout: {out}");
        assert_eq!(
            err, "openwiz: unknown command send (see openwiz --help)\n",
            "openwiz {args:?}"
        );
    }
    assert!(
        fake.invocations().is_empty(),
        "an unknown command runs no room-cli: {:?}",
        fake.invocations()
    );
    for flag in ["--help", "-h"] {
        let (code, out, err) = fake.program_cli(&[flag]);
        assert_eq!(code, 0, "{err}");
        assert!(
            !out.to_lowercase().contains("send"),
            "{flag}: help mentions send:\n{out}"
        );
        assert!(out.contains("read-only"), "{flag}:\n{out}");
    }
}

/// A switch takes
/// no value, so `--hidden=no` is a usage error, not `--hidden`.
#[test]
fn switches_take_no_value() {
    for (i, args) in [
        &["ls", "--hidden=no"][..],
        &["ls", "--json=false"][..],
        &["ls", "-l", "--hidden=yes"][..],
        &["show", "thr_syn_0001", "--json=yes"][..],
    ]
    .iter()
    .enumerate()
    {
        let fake = Fake::new(&format!("p04_switch_value_{i}"), "normal");
        let (code, out, err) = fake.cli(args);
        assert_eq!(code, 2, "openwiz {args:?}: {err}");
        assert!(out.is_empty(), "{args:?}: {out}");
        assert!(fake.invocations().is_empty(), "{args:?} reads nothing");
    }
}

/// A response over
/// the size bound fails as malformed, also when the program stops with an
/// error once its output is no longer read. The program here is a shell
/// script, not the fake: it prints 40 MB through `head`.
#[test]
fn an_oversized_response_fails_as_malformed() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("p04_oversized_response");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let program = dir.join("room-cli");
    std::fs::write(
        &program,
        "#!/bin/sh\ncase \"$1\" in\n  --version) echo 0.43.1 ;;\n  *) yes | head -c 40000000 ;;\nesac\n",
    )
    .expect("script");
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let source = openwiz_adapter_cloudroom::source::CloudRoomSource::new(&program);
    // A process another test forks while this script is still open for
    // writing can make its first starts fail with ETXTBSY ("Text file busy").
    // That is a race in the test, not the behaviour under test: retry it.
    let mut result = openwiz_core::ops::list(&source, &clock(), &ListRequest::default());
    for _ in 0..50 {
        match &result {
            Err(e) if format!("{e:?}").contains("os error 26") => {
                std::thread::sleep(Duration::from_millis(20));
                result = openwiz_core::ops::list(&source, &clock(), &ListRequest::default());
            }
            _ => break,
        }
    }
    match result {
        Err(OpError::ListFailed(SourceError::Malformed { detail })) => {
            assert!(detail.contains("more than"), "{detail}")
        }
        other => panic!("expected an explicit malformed error, got {other:?}"),
    }
}
