//! Structure: no reachable behaviour yet (sheet the check), every label the core
//! can serialize is in the schema, and the runner scrubs every call.

use std::fs;
use std::path::Path;

use openwiz_core::messaging::send::UncertainReason;
use openwiz_core::new_thread::effects::EffectKind;
use openwiz_core::new_thread::options::{NoOptionsReason, UnavailableReason};
use openwiz_core::new_thread::outcome::{FailReason, RefusalReason, Status};
use serde_json::Value;

use crate::support::*;

fn rust_text(dir: &str) -> String {
    let mut out = String::new();
    let mut stack = vec![Path::new(ROOT).join(dir)];
    while let Some(path) = stack.pop() {
        if path.is_dir() {
            for entry in fs::read_dir(&path).unwrap() {
                stack.push(entry.unwrap().path());
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push_str(&fs::read_to_string(&path).unwrap());
        }
    }
    out
}

/// A1 phase 2 reaches new threads from the CLI, the store and the
/// allowlist; since A2  also from the server's routes, through the
/// core's operations only, never an allowlist invocation of its own. The
/// fake allows `spawn` only in its one shape (N16). A tripwire, not a proof.
/// Updated for A2, with that reason.
#[test]
fn only_the_cli_and_the_store_reach_new_threads() {
    let allowlist =
        fs::read_to_string(Path::new(ROOT).join("crates/adapters/cloudroom/src/allowlist.rs"))
            .unwrap();
    // `cloud` twice: `cloud status`, and `spawn --machine cloud`.
    for (word, times) in [("\"spawn\"", 1), ("\"status\"", 1), ("\"cloud\"", 2)] {
        assert_eq!(
            allowlist.matches(word).count(),
            times,
            "allowlist.rs names {word}"
        );
    }
    for (dir, word) in [
        ("crates/cli/src", "new_thread"),
        ("crates/store/src", "NewThreadStore"),
    ] {
        assert!(rust_text(dir).contains(word), "{dir} uses {word}");
    }
    let server = rust_text("crates/server/src");
    for word in ["ThreadSpawn", "CloudStatus", "spawn_argv"] {
        assert!(!server.contains(word), "the server uses {word}");
    }
}

#[test]
fn no_new_thread_or_prompt_type_has_a_reader() {
    let read =
        fs::read_to_string(Path::new(ROOT).join("crates/core/src/new_thread/read.rs")).unwrap();
    let mut text = rust_text("crates/core/src/new_thread").replace(&read, "");
    text.push_str(&fs::read_to_string(Path::new(ROOT).join("crates/core/src/prompt.rs")).unwrap());
    let readers = text.matches("Deserialize").count();
    let guard = text.matches("serde::de::DeserializeOwned").count();
    assert_eq!(readers, guard, "a reader besides the guard's bound");
    assert_eq!(guard, 1, "the compile-time guard is in place");
    assert_eq!(
        read.matches("impl<'de> Deserialize<'de> for ").count(),
        1,
        "one reader in read.rs"
    );
    assert!(read.contains("impl<'de> Deserialize<'de> for StrictJson"));
    for word in ["try_from", "TryFrom", "FromStr", "deserialize_with"] {
        assert!(!text.contains(word), "{word}");
    }
    // read.rs converts integers with `try_from`; no serde conversion.
    for word in ["serde(", "TryFrom<String>", "FromStr", "deserialize_with"] {
        assert!(!read.contains(word), "read.rs: {word}");
    }
    for line in text.lines().chain(read.lines()) {
        assert!(
            !(line.contains("derive(") && line.contains("Deserialize")),
            "a derived reader: {line}"
        );
    }
}

/// The runner passes every call through the scrub (a tripwire).
#[test]
fn the_runner_scrubs_every_call() {
    let process =
        fs::read_to_string(Path::new(ROOT).join("crates/adapters/cloudroom/src/process.rs"))
            .unwrap();
    assert_eq!(
        process.matches("Command::new(").count(),
        1,
        "one way to start a call"
    );
    assert!(process.contains("scrub(&mut Command::new(program))"));
}

macro_rules! labels {
    ($($item:expr),* $(,)?) => {
        vec![$(serde_json::to_value($item).unwrap()),*]
    };
}

#[test]
fn every_serialized_label_is_in_the_schema() {
    let schema = schema();
    let enums = |path: &str| -> Vec<Value> {
        schema
            .pointer(path)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_else(|| panic!("no enum at {path}"))
    };
    let check = |path: &str, got: Vec<Value>| {
        let want = enums(path);
        for label in &got {
            assert!(want.contains(label), "{label} is not in {path}");
        }
        assert_eq!(want.len(), got.len(), "{path} lists only real labels");
    };
    check(
        "/$defs/refusal_reason/enum",
        labels![
            RefusalReason::NoOptions,
            RefusalReason::HarnessNotOffered,
            RefusalReason::ModelNotOffered,
            RefusalReason::EffortNotOffered,
            RefusalReason::ProjectNotFound,
            RefusalReason::ProjectAmbiguous,
            RefusalReason::ProjectPersonal,
            RefusalReason::ProjectNotStandard,
            RefusalReason::ProjectNoGithubRemote,
            RefusalReason::ProjectRemoteNotConfirmed,
            RefusalReason::ProviderUnsupported,
            RefusalReason::ProviderModelUnavailable,
            RefusalReason::ProviderEffortUnavailable,
            RefusalReason::ProviderMessageTooLong,
            RefusalReason::ProviderMessageEmpty,
            RefusalReason::ProviderProjectNotFound,
            RefusalReason::ProviderRequestIdConflict,
            RefusalReason::ProviderDiscoveryUnavailable,
        ],
    );
    check(
        "/$defs/fail_reason/enum",
        labels![
            FailReason::InterruptedBeforeDispatch,
            FailReason::ProjectsNotRead,
            FailReason::RecordNotWritten,
            FailReason::BudgetExhausted,
            FailReason::CallNotStarted,
        ],
    );
    check(
        "/$defs/uncertain_reason/enum",
        labels![
            UncertainReason::TimedOut,
            UncertainReason::EndedWithoutExitCode,
            UncertainReason::NotJson,
            UncertainReason::UnexpectedReceipt,
            UncertainReason::UndocumentedError,
            UncertainReason::CallNotConfirmed,
            UncertainReason::InterruptedAfterDispatchMark,
        ],
    );
    check(
        "/$defs/status/enum",
        labels![
            Status::Created,
            Status::Refused,
            Status::Failed,
            Status::Uncertain,
            Status::InProgress,
            Status::ConflictingReuse,
        ],
    );
    check(
        "/$defs/effect/properties/kind/enum",
        labels![
            EffectKind::CreatesAndStarts,
            EffectKind::StartsFromGithub,
            EffectKind::CopiesEnvFiles,
            EffectKind::FullPermissions,
            EffectKind::UsesCloudAgent,
            EffectKind::MacStaysAwake,
            EffectKind::CloudRoomNamesIt,
        ],
    );
    check(
        "/$defs/unavailable_harness/properties/reason/enum",
        labels![
            UnavailableReason::CatalogueNotReported,
            UnavailableReason::IdMappingNotVerified,
            UnavailableReason::NoOfferedEffort,
        ],
    );
    check(
        "/$defs/options/properties/state/oneOf/1/properties/reason/enum",
        labels![
            NoOptionsReason::NotReady,
            NoOptionsReason::HarnessesNotReported,
            NoOptionsReason::ReadFailed,
            NoOptionsReason::UnexpectedShape,
        ],
    );
}
