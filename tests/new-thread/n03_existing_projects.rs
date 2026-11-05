//! N3 Existing projects only (sheet the check): the project is read with `project
//! list` in the same run, and only a `standard` project with a GitHub remote
//! is accepted. Unknown, hidden, personal, no-remote, other-host and
//! ambiguous projects are refused with zero `spawn` calls, through the
//! release build and the in-process CLI.

use openwiz_adapter_cloudroom::new_thread::{is_github_remote, parse_projects};
use openwiz_core::model::Fact;
use serde_json::json;

use crate::fake::*;

fn refused(fake: &NewFake, args: &[&str], reason: &str) {
    let (code, json) = fake.cli_json(args, "");
    assert_eq!(code, 5, "{args:?}: {json}");
    assert_eq!(json["status"], "refused", "{json}");
    let outcome = &json["result"]["outcome"];
    assert_eq!(outcome["kind"], "refused", "{json}");
    assert_eq!(outcome["reason"], reason, "{args:?}: {json}");
    assert!(
        outcome["text"]
            .as_str()
            .unwrap()
            .contains("nothing was created"),
        "{json}"
    );
}

#[test]
fn only_a_standard_project_with_a_github_remote_is_accepted() {
    let fake = NewFake::new("n03_refusals");
    let by_name = |name: &'static str| new_args(name, "op-n03", &[], "Synthetic");
    let by_id = |id: &'static str| new_args_by_id(id, "op-n03", &[], "Synthetic");
    for (i, (args, reason)) in [
        (by_id("prj_syn_9999"), "project_not_found"),
        (by_name("Synthetic nowhere"), "project_not_found"),
        // Hidden projects are never read for a create.
        (by_id("prj_syn_0007"), "project_not_found"),
        (by_name("Synthetic hidden"), "project_not_found"),
        (by_id("prj_syn_0003"), "project_personal"),
        (by_id("prj_syn_0002"), "project_no_github_remote"),
        (by_id("prj_syn_0006"), "project_no_github_remote"),
        (by_id("prj_syn_0008"), "project_no_github_remote"),
        (by_name("Synthetic shared"), "project_ambiguous"),
    ]
    .into_iter()
    .enumerate()
    {
        let op = format!("op-n03-refused-{i}");
        let args: Vec<&str> = args
            .iter()
            .map(|a| if *a == "op-n03" { op.as_str() } else { a })
            .collect();
        refused(&fake, &args, reason);
    }
    assert_eq!(fake.spawn_count(), 0, "no refusal made a call");
    assert!(fake.spawns().is_empty());
    // Each refusal read the project list itself, in its own run, and the
    // hidden list never.
    let lists = fake
        .invocations()
        .iter()
        .filter(|a| starts(a, &["project", "list"]))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(lists.len(), 9, "{lists:?}");
    assert!(lists.iter().all(|a| !a.iter().any(|w| w == "--hidden")));
}

#[test]
fn a_shared_name_is_resolved_only_by_its_qualified_form_or_its_id() {
    let fake = NewFake::with(
        "n03_shared",
        vec![json!({
            "any_spawn": true,
            "stdout": json!({"id": "thr_syn_0024", "projectId": "prj_syn_0004", "status": "pending"})
                .to_string(),
            "creates": true,
        })],
    );
    // `~<last 6 characters of the id>`, as the inventory shows it.
    let (code, json) = fake.cli_json(
        &new_args("Synthetic shared~n_0004", "op-n03-shared", &[], "Synthetic"),
        "",
    );
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["project"]["id"], "prj_syn_0004");
    assert_eq!(fake.spawns()[0][3], "prj_syn_0004");
}

#[test]
fn a_project_list_that_cannot_be_read_creates_nothing() {
    let fake = NewFake::with(
        "n03_list_fails",
        vec![json!({
            "argv": ["project", "list", "--json", "--include-personal"],
            "stderr": "Error: HTTP 500: Synthetic server error",
            "exit": 1,
        })],
    );
    let (code, json) = fake.cli_json(
        &new_args("Synthetic project one", "op-n03-fails", &[], "Synthetic"),
        "",
    );
    assert_eq!(code, 5, "{json}");
    assert_eq!(json["result"]["outcome"]["reason"], "projects_not_read");
    assert_eq!(fake.spawn_count(), 0);
}

#[test]
fn the_github_rule() {
    for url in [
        github("0001"),
        "https://github.com/a/b".to_owned(),
        "git@github.com:synthetic-owner/repo.git".to_owned(),
        "ssh://git@github.com/synthetic-owner/repo.git".to_owned(),
    ] {
        assert!(is_github_remote(&url), "{url}");
    }
    for url in [
        "",
        "https://github.com/",
        "https://github.com",
        "https://github.com.example.invalid/a/b",
        "https://example.invalid/github.com/a/b",
        "http://github.com/a/b",
        "https://GITHUB.COM.example.invalid/a",
        "https://user@github.com/a/b",
        "git@github.com:",
        "github.com/a/b",
        "/synthetic/local/repo",
        "file:///synthetic/repo",
        " https://github.com/a/b",
    ] {
        assert!(!is_github_remote(url), "{url:?}");
    }
}

#[test]
fn projects_are_read_with_their_kind_and_remote() {
    let rows = parse_projects(project_list().to_string().as_bytes()).expect("parsed");
    let find = |id: &str| rows.iter().find(|p| p.id == id).unwrap().clone();
    assert_eq!(find("prj_syn_0001").github_remote, Fact::Known(true));
    assert_eq!(
        find("prj_syn_0001").kind,
        Fact::Known("standard".to_owned())
    );
    assert_eq!(find("prj_syn_0002").github_remote, Fact::Known(false));
    assert_eq!(find("prj_syn_0004").github_remote, Fact::Known(true));
    assert_eq!(find("prj_syn_0006").github_remote, Fact::Known(false));
    assert_eq!(
        find("prj_syn_0003").kind,
        Fact::Known("personal".to_owned())
    );
    // Drift: a missing or mistyped field is unknown, so the project is
    // refused; a row without an id can't be named.
    let drift = json!([
        {"id": "prj_syn_0011", "kind": "standard", "name": "Synthetic a"},
        {"id": "prj_syn_0012", "kind": 3, "name": "Synthetic b", "gitRemoteUrl": github("0012")},
        {"id": "prj_syn_0013", "kind": "standard", "name": "Synthetic c", "gitRemoteUrl": 7},
        {"kind": "standard", "name": "Synthetic d", "gitRemoteUrl": github("0014")},
        "not a row",
    ]);
    let rows = parse_projects(drift.to_string().as_bytes()).expect("parsed");
    assert_eq!(rows.len(), 3, "{rows:?}");
    assert!(matches!(rows[0].github_remote, Fact::Unknown(_)));
    assert!(matches!(rows[1].kind, Fact::Unknown(_)));
    assert!(matches!(rows[2].github_remote, Fact::Unknown(_)));
    assert!(parse_projects(b"{}").is_err(), "not a list");
    assert!(parse_projects(b"not json").is_err());
}
