//! Structure: the new store crate's dependencies point inward, the core
//! does no I/O, and the schema is one the in-repo checker fully enforces.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::support::*;

fn graph() -> BTreeMap<String, BTreeSet<String>> {
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--locked", "--offline"])
        .current_dir(ROOT)
        .output()
        .expect("run cargo metadata");
    assert!(output.status.success());
    let meta: Value = serde_json::from_slice(&output.stdout).unwrap();
    let names: BTreeMap<&str, &str> = meta["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["id"].as_str().unwrap(), p["name"].as_str().unwrap()))
        .collect();
    meta["resolve"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| {
            let deps = node["deps"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|d| {
                    d["dep_kinds"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|k| k["kind"].is_null())
                })
                .map(|d| names[d["pkg"].as_str().unwrap()].to_owned())
                .collect();
            (names[node["id"].as_str().unwrap()].to_owned(), deps)
        })
        .collect()
}

fn reaches(g: &BTreeMap<String, BTreeSet<String>>, from: &str) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![from.to_owned()];
    while let Some(next) = stack.pop() {
        for dep in g.get(&next).into_iter().flatten() {
            if seen.insert(dep.clone()) {
                stack.push(dep.clone());
            }
        }
    }
    seen
}

#[test]
fn the_store_depends_only_inward() {
    let g = graph();
    let store = reaches(&g, "openwiz-store");
    assert!(store.contains("openwiz-core"), "{store:?}");
    for outer in ["openwiz-adapter-cloudroom", "openwiz-cli"] {
        assert!(!store.contains(outer), "the store reaches {outer}");
    }
    for inner in ["openwiz-core", "openwiz-adapter-cloudroom"] {
        assert!(
            !reaches(&g, inner).contains("openwiz-store"),
            "{inner} reaches the store"
        );
    }
    let cli = reaches(&g, "openwiz-cli");
    assert!(cli.contains("openwiz-store") && cli.contains("openwiz-core"));
    // Every crate outside the workspace is one 0.0.1 recorded and approved.
    let recorded = fs::read_to_string(Path::new(ROOT).join("docs/dependencies.md")).unwrap();
    let approved: BTreeSet<String> = recorded
        .lines()
        .filter_map(|l| l.strip_prefix("| `"))
        .filter_map(|l| l.split('`').next())
        .map(str::to_owned)
        .collect();
    assert!(approved.contains("serde_json"), "{approved:?}");
    for name in g.keys().filter(|k| !k.starts_with("openwiz")) {
        assert!(approved.contains(name), "{name} is not a recorded crate");
    }
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn the_core_does_no_io() {
    let mut files = Vec::new();
    rust_files(&Path::new(ROOT).join("crates/core/src"), &mut files);
    assert!(files.iter().any(|f| f.ends_with("messaging/send.rs")));
    for path in files {
        let text = fs::read_to_string(&path).unwrap();
        for io in [
            "std::fs",
            "std::io",
            "std::net",
            "std::process",
            "std::env",
            "std::os",
            "SystemTime",
            "Instant::now",
            "println!",
            "eprintln!",
        ] {
            assert!(!text.contains(io), "{path:?} uses {io}");
        }
    }
}

#[test]
fn the_messaging_schema_uses_only_checked_keywords() {
    let schema = messaging_schema();
    assert_eq!(schema["title"], "openwiz.messaging.v1");
    assert_eq!(crate::schema::lint(&schema), Vec::<String>::new());
    let errors = validate_report(&serde_json::json!({}));
    assert!(
        errors.iter().all(|e| !e.contains("unresolvable")),
        "{errors:#?}"
    );
    for def in ["request", "record", "effects", "delivery"] {
        assert!(schema["$defs"][def].is_object(), "{def}");
    }
    let request = serde_json::json!({
        "target": "thr_syn_0001",
        "mode": "queue",
        "text": "Synthetic",
        "op_id": "op-1",
        "resume_paused_queue": false,
        "accept_possible_duplicate": false,
    });
    assert!(validate_def("request", &request).is_empty());
    let mut auto = request.clone();
    auto["mode"] = "auto".into();
    assert!(
        !validate_def("request", &auto).is_empty(),
        "auto is never a mode"
    );
}

#[test]
fn every_serialized_label_is_in_the_schema() {
    use openwiz_core::messaging::send::{FailReason, Status, UncertainReason, WaitingOn};
    let schema = messaging_schema();
    let enums = |path: &str| -> Vec<Value> {
        schema
            .pointer(path)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_else(|| panic!("no enum at {path}"))
    };
    let fail = enums("/$defs/fail_reason/enum");
    for reason in [
        FailReason::NotFound,
        FailReason::InvalidInput,
        FailReason::UnsupportedMode,
        FailReason::InterruptedBeforeDispatch,
        FailReason::UntestedVersion,
        FailReason::TargetNotRead,
        FailReason::TargetArchived,
        FailReason::TargetHidden,
        FailReason::TargetVisibilityUnknown,
        FailReason::PausedQueueNotAcknowledged,
        FailReason::RecordNotWritten,
        FailReason::BudgetExhausted,
        FailReason::CallNotStarted,
    ] {
        let label = serde_json::to_value(reason).unwrap();
        assert!(fail.contains(&label), "{label} is not in the schema");
    }
    let uncertain = enums("/$defs/delivery/oneOf/3/properties/reason/enum");
    for reason in [
        UncertainReason::TimedOut,
        UncertainReason::EndedWithoutExitCode,
        UncertainReason::NotJson,
        UncertainReason::UnexpectedReceipt,
        UncertainReason::UndocumentedError,
        UncertainReason::CallNotConfirmed,
        UncertainReason::InterruptedAfterDispatchMark,
    ] {
        let label = serde_json::to_value(reason).unwrap();
        assert!(uncertain.contains(&label), "{label} is not in the schema");
    }
    let waiting = enums("/$defs/delivery/oneOf/0/properties/waiting_on/enum");
    for kind in [
        WaitingOn::Time,
        WaitingOn::ThreadBusy,
        WaitingOn::TurnStarting,
        WaitingOn::Provisioning,
        WaitingOn::HostOffline,
        WaitingOn::Interaction,
        WaitingOn::Plugin,
    ] {
        let label = serde_json::to_value(kind).unwrap();
        assert!(waiting.contains(&label), "{label} is not in the schema");
    }
    let status = enums("/$defs/report/properties/status/enum");
    for s in [
        Status::Queued,
        Status::Accepted,
        Status::Failed,
        Status::Uncertain,
        Status::InProgress,
        Status::ConflictingReuse,
        Status::PossibleDuplicateRefused,
    ] {
        let label = serde_json::to_value(s).unwrap();
        assert!(status.contains(&label), "{label} is not in the schema");
    }
    // A label added to the core without a schema entry fails to compile here.
    fn _exhaustive(r: FailReason) {
        match r {
            FailReason::NotFound
            | FailReason::InvalidInput
            | FailReason::UnsupportedMode
            | FailReason::InterruptedBeforeDispatch
            | FailReason::UntestedVersion
            | FailReason::TargetNotRead
            | FailReason::TargetArchived
            | FailReason::TargetHidden
            | FailReason::TargetVisibilityUnknown
            | FailReason::PausedQueueNotAcknowledged
            | FailReason::RecordNotWritten
            | FailReason::BudgetExhausted
            | FailReason::CallNotStarted => {}
        }
    }
}

/// Test isolation: no messaging test path under the system temp directory
/// is fixed across test runs. Two suites running at once (two target
/// directories on one machine) must never share send records, or one wipes
/// the other's claim mid-test. The system temp directory may be read only in
/// `support::run_root`, which must name the run's own directory after this
/// process's id and the time, and fail rather than reuse one. Every file
/// here is scanned, this one too; the patterns are built with `concat!` so
/// this check's own text does not match.
#[test]
fn no_temp_path_is_shared_between_test_runs() {
    let temp = concat!("temp_", "dir()");
    let required = [
        "let nanos = SystemTime::now()",
        concat!(".map_or(0, |d| d.as_", "nanos());"),
        concat!(
            "let root = parent.join(format!(\"{}-{nanos}\", std::process::",
            "id()));"
        ),
        concat!("crate::dirs::private_", "dir(&root).expect("),
    ];
    let dir = Path::new(ROOT).join("tests/messaging");
    let mut found = Vec::new();
    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        let allowed = if name == "support.rs" {
            text.find("fn run_root(")
                .map(|start| &text[start..start + text[start..].find("\n}\n").unwrap_or(0)])
        } else {
            None
        };
        if let Some(body) = allowed {
            for line in required {
                if !body.lines().any(|l| l.trim().starts_with(line)) {
                    found.push(format!("support::run_root has no line starting {line:?}"));
                }
            }
        } else if name == "support.rs" {
            found.push("support.rs has no run_root".to_owned());
        }
        let outside =
            text.matches(temp).count() - allowed.map_or(0, |body| body.matches(temp).count());
        if outside > 0 {
            found.push(format!(
                "{name}: {temp} used {outside} time(s) outside support::run_root"
            ));
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
}

#[test]
fn the_preview_and_lookup_shapes_are_in_the_schema() {
    use openwiz_core::messaging::lookup::{LOOKUP_SCHEMA_ID, LookupResult};
    use openwiz_core::messaging::preview::{PREVIEW_SCHEMA_ID, PreviewStatus};
    let schema = messaging_schema();
    for def in ["preview", "lookup", "refusal"] {
        assert!(schema["$defs"][def].is_object(), "no $defs/{def}");
    }
    assert_eq!(
        schema["$defs"]["preview"]["properties"]["schema"]["const"],
        PREVIEW_SCHEMA_ID
    );
    assert_eq!(
        schema["$defs"]["lookup"]["properties"]["schema"]["const"],
        LOOKUP_SCHEMA_ID
    );
    let statuses = schema["$defs"]["preview"]["properties"]["status"]["enum"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for s in [
        PreviewStatus::Ready,
        PreviewStatus::NeedsAcknowledgement,
        PreviewStatus::Refused,
    ] {
        let label = serde_json::to_value(s).unwrap();
        assert!(statuses.contains(&label), "{label} is not in the schema");
    }
    fn _exhaustive(s: PreviewStatus, r: LookupResult) {
        match s {
            PreviewStatus::Ready | PreviewStatus::NeedsAcknowledgement | PreviewStatus::Refused => {
            }
        }
        match r {
            LookupResult::NoRecord | LookupResult::OtherTarget | LookupResult::Recorded { .. } => {}
        }
    }
    for (result, ok) in [
        (serde_json::json!({"kind": "no_record"}), true),
        (serde_json::json!({"kind": "other_target"}), true),
        (serde_json::json!({"kind": "recorded"}), false),
        (serde_json::json!({"kind": "dispatched"}), false),
    ] {
        let lookup = serde_json::json!({
            "schema": LOOKUP_SCHEMA_ID,
            "op_id": "op-1",
            "target": "thr_syn_0001",
            "summary": "Synthetic",
            "result": result,
        });
        assert_eq!(validate_def("lookup", &lookup).is_empty(), ok, "{lookup}");
    }
    let earlier = &schema["$defs"]["earlier_send"];
    assert!(
        earlier["required"]
            .as_array()
            .is_some_and(|r| r.contains(&"in_flight".into())),
        "{earlier}"
    );
}

/// Sheet U8: the route body never carries the target; it comes only from
/// the path. The schema's request no longer says otherwise.
#[test]
fn the_request_no_longer_says_the_route_body_carries_the_target() {
    let schema = messaging_schema();
    let words = schema["$defs"]["request"]["description"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        !words.contains("route body carries these fields"),
        "{words}"
    );
    for needed in ["path", "never the body"] {
        assert!(words.contains(needed), "{needed:?} not in: {words}");
    }
    for field in [
        "op_id",
        "mode",
        "text",
        "resume_paused_queue",
        "accept_possible_duplicate",
    ] {
        assert!(words.contains(field), "{field} not in: {words}");
    }
}

/// Fail closed : the store methods the in-flight guard, the
/// lookup and the withdrawal depend on have no default body, so every store
/// must implement them; none can fall back to "nothing in flight".
#[test]
fn the_guard_methods_have_no_default() {
    let text =
        fs::read_to_string(Path::new(ROOT).join("crates/core/src/messaging/record.rs")).unwrap();
    let start = text.find("pub trait SendStore").expect("the trait");
    let body = &text[start..start + text[start..].find("\n}\n").unwrap()];
    for method in [
        "fn uncertain_matching(",
        "fn in_flight_matching(",
        "fn inspect(",
        "fn withdraw(",
    ] {
        let at = body.find(method).unwrap_or_else(|| panic!("no {method}"));
        let end = body[at..].find([';', '{']).unwrap();
        assert_eq!(
            &body[at + end..=at + end],
            ";",
            "{method} has a default body"
        );
    }
}
