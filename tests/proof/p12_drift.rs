//! P12 Version and schema drift: an untested `room-cli` version and missing
//! or mistyped fields are reported explicitly.

use openwiz_core::envelope::{Completeness, GapKind};
use openwiz_core::model::Fact;
use openwiz_core::ops::ListRequest;

use crate::support::*;

/// The `room-cli` version is never a gate: `room-cli` updates often. Any
/// version is read, shown and the run goes on; one other than the
/// live-tested version is marked untested. The live-tested version is v127,
/// so 0.43.1 is untested too.
#[test]
fn an_untested_version_is_reported_and_never_stops() {
    for (scenario, version) in [("version-0.44.0", "0.44.0"), ("version-0.43.1", "0.43.1")] {
        for (name, request) in [("ls", ListRequest::default()), ("ls_long", long())] {
            let fake = Fake::new(&format!("p12_untested_{scenario}_{name}"), scenario);
            let env = fake
                .list(&request)
                .unwrap_or_else(|e| panic!("{scenario} {name}: still lists, got {e:?}"));
            assert_eq!(env.source.version, Fact::Known(version.to_owned()));
            assert_eq!(env.source.tested_version, "v127");
            assert!(!env.source.version_tested, "{scenario}: marked untested");
            assert!(!env.data.sessions.is_empty(), "{scenario}: sessions listed");
        }
        let fake = Fake::new(&format!("p12_untested_{scenario}_show"), scenario);
        let env = fake
            .show("thr_syn_0001")
            .unwrap_or_else(|e| panic!("{scenario} show: still shows, got {e:?}"));
        assert!(!env.source.version_tested);

        for args in [
            &["ls"][..],
            &["ls", "-l"][..],
            &["show", "thr_syn_0001"][..],
        ] {
            let fake = Fake::new(
                &format!("p12_untested_cli_{scenario}_{}", args.join("_")),
                scenario,
            );
            let (code, out, err) = fake.cli(args);
            assert_eq!(code, 0, "{scenario} openwiz {args:?}: {err}");
            let header = out.lines().next().unwrap_or_default();
            assert!(
                header.contains(&format!("room-cli {version} (untested")),
                "{scenario} {args:?} header: {header}"
            );
            assert!(out.contains("thr_syn_0001"), "{scenario} {args:?}: {out}");
        }
    }
}

/// Version text that is neither `N.N.N` nor `v<N>` is never echoed; the
/// version is unknown with a reason, a gap says so, and the run goes on.
#[test]
fn an_unrecognised_version_is_unknown_and_never_stops() {
    let fake = Fake::new("p12_unrecognised_version", "version-unrecognised");
    let env = fake.list(&ListRequest::default()).expect("still lists");
    unknown_reason(&env.source.version);
    assert!(!env.source.version_tested);
    assert!(
        env.gaps.iter().any(|g| g.kind == GapKind::UntestedVersion),
        "{:?}",
        env.gaps
    );
    assert!(!env.data.sessions.is_empty());
    let (code, out, err) = fake.cli(&["ls", "--json"]);
    assert_eq!(code, 0, "{err}");
    assert!(!out.contains("Synthetic not a version"), "{out}");
}

/// A failed version read is unknown with a controlled reason. The source's
/// own error text is never echoed in text, JSON or stderr, even a line that
/// passes the stderr sanitizer's general grammar, and the run goes on.
#[test]
fn a_failed_version_read_never_echoes_its_error_text() {
    const MARKER: &str = "Synthetic private version marker";
    let fake = Fake::new("p12_failed_version_core", "version-failed");
    let env = fake.list(&ListRequest::default()).expect("still lists");
    let reason = unknown_reason(&env.source.version).to_owned();
    assert!(!reason.contains(MARKER), "{reason}");
    assert!(
        reason.contains("exited 1 (detail withheld)"),
        "the safe fact is kept: {reason}"
    );
    assert!(
        env.gaps.iter().all(|g| !g.detail.contains(MARKER)),
        "{:?}",
        env.gaps
    );
    assert!(env.gaps.iter().any(|g| g.kind == GapKind::UntestedVersion));
    let shown = fake.show("thr_syn_0001").expect("still shows");
    assert!(matches!(shown.source.version, Fact::Unknown(_)));
    for args in [
        &["ls"][..],
        &["ls", "--json"][..],
        &["ls", "-l"][..],
        &["show", "thr_syn_0001"][..],
        &["show", "thr_syn_0001", "--json"][..],
    ] {
        let fake = Fake::new(
            &format!("p12_failed_version_cli_{}", args.join("_")),
            "version-failed",
        );
        let (code, out, err) = fake.cli(args);
        assert_eq!(code, 0, "openwiz {args:?}: {err}");
        assert!(
            !out.contains(MARKER) && !err.contains(MARKER),
            "openwiz {args:?}: {out}{err}"
        );
        assert!(
            out.contains("exited 1 (detail withheld)"),
            "openwiz {args:?}: {out}"
        );
    }
}

#[test]
fn missing_fields_are_reported_by_session_and_field() {
    let fake = Fake::new("p12_missing_fields", "thread-list-missing-fields");
    let env = fake
        .list(&ListRequest::default())
        .expect("honest partial list");
    assert_eq!(env.coverage.state, Completeness::Partial);
    for (id, field) in [
        ("thr_syn_0002", "status"),
        ("thr_syn_0003", "parentThreadId"),
        ("thr_syn_0004", "updatedAt"),
        ("thr_syn_0005", "projectId"),
    ] {
        assert!(
            env.gaps.iter().any(|g| g.kind == GapKind::MissingField
                && g.subject.as_deref().is_some_and(|s| s.contains(id))
                && g.detail.contains(field)),
            "no missing-field gap for {id} {field}: {:?}",
            env.gaps
        );
    }
}

#[test]
fn mistyped_fields_are_reported_by_session_and_field() {
    let fake = Fake::new("p12_mistyped_fields", "thread-list-wrong-types");
    let env = fake
        .list(&ListRequest::default())
        .expect("honest partial list");
    for (id, field) in [
        ("thr_syn_0001", "status"),
        ("thr_syn_0002", "updatedAt"),
        ("thr_syn_0003", "projectId"),
        ("thr_syn_0004", "purpose.text"),
        ("thr_syn_0005", "parentThreadId"),
        ("thr_syn_0006", "executionTarget"),
    ] {
        assert!(
            env.gaps.iter().any(|g| g.kind == GapKind::WrongType
                && g.subject.as_deref().is_some_and(|s| s.contains(id))
                && g.detail.contains(field)),
            "no wrong-type gap for {id} {field}: {:?}",
            env.gaps
        );
    }
}

#[test]
fn unknown_extra_fields_are_tolerated() {
    let normal = Fake::new("p12_extra_fields_normal", "normal")
        .list(&long())
        .expect("normal list");
    let extra = Fake::new("p12_extra_fields", "unknown-fields")
        .list(&long())
        .expect("list with extra fields");
    assert_eq!(extra.coverage.state, Completeness::Complete);
    assert_eq!(extra.data, normal.data, "extra fields change nothing");
}

#[test]
fn missing_detail_fields_are_reported_by_session_and_field() {
    let fake = Fake::new("p12_missing_detail_fields", "show-missing-fields");
    let env = fake.list(&long()).expect("ls -l");
    for (id, field) in [
        ("thr_syn_0001", "model"),
        ("thr_syn_0006", "reasoningLevel"),
    ] {
        assert!(
            env.gaps.iter().any(|g| g.kind == GapKind::MissingField
                && g.subject.as_deref().is_some_and(|s| s.contains(id))
                && g.detail.contains(field)),
            "no missing-field gap for {id} {field}: {:?}",
            env.gaps
        );
    }
}

/// A copy of the `normal` scenario in the fake's own directory, with
/// `thread list --json` answered by `edit`ed rows.
fn normal_with_thread_list(test: &str, edit: impl Fn(&mut serde_json::Value)) -> Fake {
    use serde_json::Value;
    use std::fs;
    let fake = Fake::new(test, "normal");
    let normal = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/room-cli-0.43.1/normal"
    ));
    let read = |name: &str| -> Value {
        serde_json::from_str(&fs::read_to_string(normal.join(name)).expect(name)).expect(name)
    };
    let mut rows = read("thread-list.json");
    edit(&mut rows);
    let rows_file = fake.dir.join("thread-list-edited.json");
    fs::write(&rows_file, rows.to_string()).expect("rows");
    let mut scenario = read("scenario.json");
    for response in scenario["responses"].as_array_mut().expect("responses") {
        if response["argv"] == serde_json::json!(["thread", "list", "--json"]) {
            response["stdout_file"] = Value::from(rows_file.to_str().expect("utf-8"));
        } else if let Some(file) = response["stdout_file"].as_str() {
            let absolute = normal.join(file);
            response["stdout_file"] = Value::from(absolute.to_str().expect("utf-8"));
        }
    }
    let scenario_file = fake.dir.join("scenario.json");
    fs::write(&scenario_file, scenario.to_string()).expect("scenario");
    fs::write(
        fake.dir.join("fake-room-cli.scenario"),
        scenario_file.to_str().expect("utf-8"),
    )
    .expect("pointer");
    fake
}

/// A value of the
/// right type that 0.43.1 never had (a new `visibility` or
/// `executionTarget`) is unknown, the row stays listed, and the drift is a
/// gap, so coverage is partial.
#[test]
fn unrecognised_values_are_reported_as_drift() {
    let fake = normal_with_thread_list("p12_unrecognised_values", |rows| {
        rows[0]["visibility"] = "synthetic-private".into();
        rows[5]["executionTarget"] = "synthetic-elsewhere".into();
    });
    let env = fake
        .list(&ListRequest::default())
        .expect("honest partial list");
    assert_eq!(env.coverage.state, Completeness::Partial);
    unknown_reason(&session(&env, "thr_syn_0001").hidden);
    unknown_reason(&session(&env, "thr_syn_0006").location);
    for (id, field) in [
        ("thr_syn_0001", "visibility"),
        ("thr_syn_0006", "executionTarget"),
    ] {
        assert!(
            env.gaps.iter().any(|g| g.kind == GapKind::WrongType
                && g.subject.as_deref().is_some_and(|s| s.contains(id))
                && g.detail.contains(field)),
            "no drift gap for {id} {field}: {:?}",
            env.gaps
        );
    }
}
