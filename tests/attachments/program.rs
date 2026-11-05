//! The release `openwiz attachment add, ls, rm`, as processes: the real
//! free-space measure (`/bin/df`, sheet the check), the real clock, the owner's
//! state directory from `OPENWIZ_STATE_DIR`. No other program runs: a `df`
//! and a `room-cli` first on `PATH` are never run (AP7, AP10). Each run's
//! stderr is clean (AP12), and the store is owner-only (AP9).

use serde_json::Value;

use crate::bench::{Bench, CONTENT, assert_clean_log, mode, release_run, sha256_of, tripwires};
use crate::support::{PNG, padded};

fn json(out: &std::process::Output) -> Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout is not JSON ({e}): {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

#[test]
fn the_release_program_stages_lists_and_removes_and_runs_nothing_else() {
    let bench = Bench::new("program_release");
    let wires = tripwires(&bench);
    let photo = bench.source("syn-program-photo.png", &padded(PNG, 20_000));
    let notes = bench.source("syn-program-notes.txt", CONTENT.as_bytes());
    let folder = bench.sources.join("syn-program-folder");
    crate::dirs::private_dir(&folder).unwrap();
    let paths = [photo.to_str().unwrap(), notes.to_str().unwrap()];

    let added = release_run(&bench, &wires, &["add", "--json", paths[0], paths[1]]);
    assert_eq!(added.status.code(), Some(0), "{added:?}");
    let report = json(&added);
    assert_eq!(report["schema"], "openwiz.attachment-add.v1");
    let ready = report["ready"].as_array().unwrap();
    assert_eq!(ready.len(), 2, "{report:#}");
    for (entry, path) in ready.iter().zip(paths) {
        assert_eq!(
            entry["staged"]["attachment"]["sha256"],
            sha256_of(path.as_ref())
        );
    }
    let id = ready[0]["staged"]["attachment"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // A failure, in text: exit 5, the reason shown.
    let failed = release_run(&bench, &wires, &["add", folder.to_str().unwrap()]);
    assert_eq!(failed.status.code(), Some(5), "{failed:?}");
    assert!(String::from_utf8_lossy(&failed.stdout).contains(": failed (folder) · "));

    let listed = release_run(&bench, &wires, &["ls", "--json"]);
    assert_eq!(listed.status.code(), Some(0), "{listed:?}");
    assert_eq!(json(&listed)["attachments"].as_array().unwrap().len(), 2);

    let removed = release_run(&bench, &wires, &["rm", &id]);
    assert_eq!(removed.status.code(), Some(0), "{removed:?}");
    assert!(String::from_utf8_lossy(&removed.stdout).contains(&format!("{id}: removed")));
    let missing = release_run(&bench, &wires, &["rm", &id]);
    assert_eq!(missing.status.code(), Some(3), "{missing:?}");
    let usage = release_run(&bench, &wires, &["rm", "../escape"]);
    assert_eq!(usage.status.code(), Some(2), "{usage:?}");
    assert!(usage.stdout.is_empty());

    for out in [&added, &failed, &listed, &removed, &missing, &usage] {
        assert_clean_log(&String::from_utf8_lossy(&out.stderr));
    }
    for name in ["df", "room-cli"] {
        assert!(
            !wires.join(format!("{name}.ran")).exists(),
            "the {name} on PATH was run"
        );
    }
    assert_eq!(mode(&bench.state), 0o700);
    assert_eq!(mode(&bench.attachments()), 0o700);
    assert_eq!(bench.ids().len(), 1);
}
