//! P6 Verified grouping: a session is grouped only by a project id seen in
//! the same run. Anything else is ungrouped, with a reason.

use openwiz_core::model::{Fact, Grouping, ProjectId};
use openwiz_core::ops::{self, ListRequest};

use crate::support::*;

fn project_of(group: &Grouping) -> Option<&str> {
    match group {
        Grouping::Project { id, .. } => Some(id.0.as_str()),
        Grouping::Ungrouped { .. } => None,
    }
}

#[test]
fn sessions_are_grouped_by_a_project_seen_in_the_same_run() {
    let fake = Fake::new("p06_grouped_by_seen_project", "normal");
    let env = fake.list(&ListRequest::default()).expect("normal list");
    assert_eq!(
        session(&env, "thr_syn_0001").group,
        Grouping::Project {
            id: ProjectId("prj_syn_0001".to_owned()),
            name: Fact::Known("Synthetic project one".to_owned()),
        }
    );
    assert_eq!(
        project_of(&session(&env, "thr_syn_0004").group),
        Some("prj_syn_0003")
    );
}

#[test]
fn an_unseen_project_id_is_ungrouped_with_a_reason() {
    let fake = Fake::new("p06_unseen_project", "normal");
    let env = fake.list(&ListRequest::default()).expect("normal list");
    match &session(&env, "thr_syn_0003").group {
        Grouping::Ungrouped { reason } => {
            assert!(
                reason.contains("prj_syn_0099"),
                "the reason names the id: {reason}"
            )
        }
        other => panic!("prj_syn_0099 is not in the project list, got {other:?}"),
    }
}

#[test]
fn shared_project_names_never_merge_groups() {
    let fake = Fake::new("p06_shared_names", "normal");
    let env = fake.list(&ListRequest::default()).expect("normal list");
    assert_eq!(
        project_of(&session(&env, "thr_syn_0005").group),
        Some("prj_syn_0004")
    );
    assert_eq!(
        project_of(&session(&env, "thr_syn_0006").group),
        Some("prj_syn_0005")
    );
}

#[test]
fn a_missing_or_mistyped_project_id_is_ungrouped_with_a_reason() {
    for (scenario, id) in [
        ("thread-list-missing-fields", "thr_syn_0005"),
        ("thread-list-wrong-types", "thr_syn_0003"),
    ] {
        let fake = Fake::new(&format!("p06_bad_project_id_{scenario}"), scenario);
        let env = fake
            .list(&ListRequest::default())
            .expect("honest partial list");
        assert!(
            matches!(&session(&env, id).group, Grouping::Ungrouped { reason } if !reason.is_empty()),
            "{scenario} {id}: {:?}",
            session(&env, id).group
        );
    }
}

#[test]
fn text_shows_ungrouped_sessions_with_their_reason() {
    let fake = Fake::new("p06_text_ungrouped", "normal");
    let json = fake.cli_json(&["ls", "--json"]);
    let reason = json["data"]["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .find(|s| s["id"] == "thr_syn_0003")
        .and_then(|s| s["group"]["reason"].as_str())
        .expect("thr_syn_0003 is ungrouped in JSON")
        .to_owned();
    let (code, text, _) = fake.cli(&["ls"]);
    assert_eq!(code, 0);
    assert!(
        text.contains("thr_syn_0003"),
        "ungrouped sessions stay visible:\n{text}"
    );
    assert!(text.contains(&reason), "the reason is shown:\n{text}");
}

/// One source is read twice;
/// between the runs a project disappears, then comes back. Each run groups by
/// its own project list only, so a mapping kept from an earlier run is caught.
#[test]
fn grouping_uses_only_the_current_runs_project_list() {
    // Through the CloudRoom adapter and the fake: one source, two scenarios.
    let fake = Fake::new("p06_successive_runs", "normal");
    let source = fake.source();
    let first = ops::list(&source, &clock(), &ListRequest::default()).expect("first run");
    assert_eq!(
        project_of(&session(&first, "thr_syn_0001").group),
        Some("prj_syn_0001")
    );

    fake.set_scenario("project-removed");
    let second = ops::list(&source, &clock(), &ListRequest::default()).expect("second run");
    match &session(&second, "thr_syn_0001").group {
        Grouping::Ungrouped { reason } => assert!(
            reason.contains("prj_syn_0001"),
            "the reason names the id: {reason}"
        ),
        other => panic!("prj_syn_0001 is not in this run's project list, got {other:?}"),
    }
    assert_eq!(
        project_of(&session(&second, "thr_syn_0002").group),
        Some("prj_syn_0002"),
        "projects still listed stay grouped"
    );
    let (code, text, _) = cli_with(&source, &["ls"]);
    assert_eq!(code, 0);
    let groups = rows_by_group(&text);
    let header_of = |id: &str| {
        groups
            .iter()
            .find(|(row, _)| row == id)
            .and_then(|(_, header)| header.clone())
    };
    assert_eq!(
        header_of("thr_syn_0001").as_deref(),
        Some(UNGROUPED_HEADER),
        "{text}"
    );
    assert!(
        !text.contains("/factories/Synthetic project one/"),
        "a group from an earlier run is shown:\n{text}"
    );

    fake.set_scenario("normal");
    let third = ops::list(&source, &clock(), &ListRequest::default()).expect("third run");
    assert_eq!(
        project_of(&session(&third, "thr_syn_0001").group),
        Some("prj_syn_0001"),
        "an earlier run's absence is not kept either"
    );

    // Through the core alone: one scripted source whose project list changes.
    let source = ScriptedSource::new(2);
    let first = list_scripted(&source, &ListRequest::default()).expect("first run");
    assert_eq!(
        project_of(&session(&first, "thr_syn_0001").group),
        Some("prj_syn_0001")
    );
    source.set_projects(vec![]);
    let second = list_scripted(&source, &ListRequest::default()).expect("second run");
    for id in ["thr_syn_0001", "thr_syn_0002"] {
        assert!(
            matches!(&session(&second, id).group, Grouping::Ungrouped { reason } if reason.contains("prj_syn_0001")),
            "{id}: {:?}",
            session(&second, id).group
        );
    }
}

/// Shared project
/// names are shown as `name~<last 6 characters of the id>`; a unique name is
/// shown as it is. JSON keeps the reported name; the suffix is display only.
#[test]
fn shared_project_names_are_shown_with_the_last_six_characters_of_the_id() {
    let fake = Fake::new("p06_shared_name_display", "normal");
    let json = fake.cli_json(&["ls", "--json"]);
    for row in json["data"]["sessions"].as_array().expect("sessions") {
        if row["id"] == "thr_syn_0005" || row["id"] == "thr_syn_0006" {
            assert_eq!(row["group"]["name"]["known"], "Synthetic shared name");
        }
    }
    let (code, text, _) = fake.cli(&["ls"]);
    assert_eq!(code, 0);
    let groups = rows_by_group(&text);
    for (id, header) in [
        ("thr_syn_0001", "/factories/Synthetic project one/"),
        ("thr_syn_0005", "/factories/Synthetic shared name~n_0004/"),
        ("thr_syn_0006", "/factories/Synthetic shared name~n_0005/"),
    ] {
        let found = groups
            .iter()
            .find(|(row, _)| row == id)
            .and_then(|(_, h)| h.as_deref());
        assert_eq!(found, Some(header), "{id}:\n{text}");
    }
    assert!(
        !text.contains("/factories/Synthetic shared name/"),
        "a shared name is never shown bare:\n{text}"
    );
    let (code, text, _) = fake.cli(&["show", "thr_syn_0006"]);
    assert_eq!(code, 0);
    assert!(
        text.contains("/factories/Synthetic shared name~n_0005/"),
        "{text}"
    );
}

/// Text groups follow the
/// verified project id, never the printed name. A project named
/// `_ungrouped`, and two names that print the same once unsafe characters
/// are replaced, each keep their own header, distinct from the others and
/// from `/factories/_ungrouped/`.
#[test]
fn text_groups_follow_the_project_id_not_the_printed_name() {
    use openwiz_core::source::ProjectRecord;
    use std::collections::BTreeSet;
    let project = |id: &str, name: &str| ProjectRecord {
        id: ProjectId(id.to_owned()),
        name: Fact::Known(name.to_owned()),
    };
    let mut records: Vec<_> = (1..=4).map(scripted_record).collect();
    for (record, project_id) in records.iter_mut().zip([
        "prj_syn_0001",
        "prj_syn_0099",
        "prj_syn_0002",
        "prj_syn_0003",
    ]) {
        record.project_id = Fact::Known(ProjectId(project_id.to_owned()));
    }
    let source = ScriptedSource::with_records(records).with_projects(vec![
        project("prj_syn_0001", "_ungrouped"),
        project("prj_syn_0002", "Synthetic\u{202E}look-alike"),
        project("prj_syn_0003", "Synthetic\u{200B}look-alike"),
    ]);
    let (code, text, err) = cli_with(&source, &["ls"]);
    assert_eq!(code, 0, "{err}");
    let groups = rows_by_group(&text);
    let header = |id: &str| {
        groups
            .iter()
            .find(|(row, _)| row == id)
            .and_then(|(_, h)| h.clone())
            .unwrap_or_else(|| panic!("{id} has no header:\n{text}"))
    };
    assert_eq!(header("thr_syn_0002"), UNGROUPED_HEADER, "{text}");
    let headers: Vec<String> = [
        "thr_syn_0001",
        "thr_syn_0002",
        "thr_syn_0003",
        "thr_syn_0004",
    ]
    .iter()
    .map(|id| header(id))
    .collect();
    let distinct: BTreeSet<&String> = headers.iter().collect();
    assert_eq!(
        distinct.len(),
        4,
        "four groups, four headers: {headers:?}\n{text}"
    );
    let printed: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with(GROUP_ROOT) && l.ends_with('/'))
        .collect();
    assert_eq!(printed.len(), 4, "each header once:\n{text}");

    let (code, text, err) = cli_with(&source, &["show", "thr_syn_0001"]);
    assert_eq!(code, 0, "{err}");
    assert!(
        !text.lines().any(|l| l.trim() == UNGROUPED_HEADER),
        "a verified project is never shown as ungrouped:\n{text}"
    );
}

/// Headers stay
/// distinct even when a project's name is another one's widened header.
#[test]
fn group_headers_stay_distinct_when_a_name_copies_a_widened_header() {
    use openwiz_core::source::ProjectRecord;
    use std::collections::BTreeSet;
    let project = |id: &str, name: &str| ProjectRecord {
        id: ProjectId(id.to_owned()),
        name: Fact::Known(name.to_owned()),
    };
    let ids = ["prj_syn_a_000001", "prj_syn_b_000001", "prj_syn_c_000001"];
    let mut records: Vec<_> = (1..=3).map(scripted_record).collect();
    for (record, id) in records.iter_mut().zip(ids) {
        record.project_id = Fact::Known(ProjectId(id.to_owned()));
    }
    let source = ScriptedSource::with_records(records).with_projects(vec![
        project(ids[0], "Synthetic X"),
        project(ids[1], "Synthetic X"),
        project(ids[2], "Synthetic X~prj_syn_a_000001"),
    ]);
    let (code, text, err) = cli_with(&source, &["ls"]);
    assert_eq!(code, 0, "{err}");
    let headers: BTreeSet<String> = rows_by_group(&text)
        .into_iter()
        .filter_map(|(_, header)| header)
        .collect();
    assert_eq!(headers.len(), 3, "three groups, three headers:\n{text}");
}
