//! AP12 Logs are clean. OpenWiz's only log is stderr: every run in this
//! suite goes through the bench, which scans its stderr for any synthetic
//! file name, any synthetic content and any SHA-256 (`assert_clean_log`).
//! This case drives the error paths too: a store that can't be opened, one
//! that fails part way through staging, a broken record, refusals. A
//! store error names an attachment by its id, never by its file name.

use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;

use openwiz_core::attachment::Claim;

use crate::bench::{Bench, CONTENT, ClaimsOf, Free, PLENTY, T0, assert_clean_log};
use crate::support::{PNG, padded};

#[test]
fn the_scan_finds_names_content_and_hashes() {
    for planted in [
        "openwiz: could not read syn-ap12-photo.png",
        "openwiz: SYNTHETIC-CONTENT",
        "openwiz: ZZZZZZZZZZZZ",
        &format!(
            "openwiz: {}",
            openwiz_core::messaging::sha256_hex(b"synthetic")
        ),
    ] {
        let caught = std::panic::catch_unwind(|| assert_clean_log(planted));
        assert!(caught.is_err(), "the scan missed {planted:?}");
    }
    assert_clean_log("openwiz: the attachment store could not be used: att-0123 (63 hex: ok)");
}

#[test]
fn every_error_path_logs_no_name_content_or_hash() {
    let bench = Bench::new("ap12_errors");
    let photo = bench.source("syn-ap12-photo.png", &padded(PNG, 5_000));
    let notes = bench.source("syn-ap12-notes.txt", CONTENT.as_bytes());
    let paths = [photo.to_str().unwrap(), notes.to_str().unwrap()];
    let added = bench.cli(&["add", "--json", paths[0], paths[1]]);
    assert_eq!(added.code, 0, "{added:?}");
    let ids: Vec<String> = added.json()["ready"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["staged"]["attachment"]["id"].as_str().unwrap().to_owned())
        .collect();

    // Staging fails part way: its directory can't be made.
    let attachments = bench.attachments();
    fs::set_permissions(&attachments, fs::Permissions::from_mode(0o500)).unwrap();
    let failed = bench.cli(&["add", "--json", paths[0]]);
    fs::set_permissions(&attachments, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(failed.code, 10, "{failed:?}");
    let error = failed.json()["not_staged"][0]["error"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        !error.contains("syn-"),
        "a store error names the file: {error}"
    );
    assert_clean_log(&error);

    // A broken record, a refusal, an id that isn't there.
    fs::write(attachments.join(&ids[0]).join(".meta.json"), "not JSON").unwrap();
    assert_eq!(bench.cli(&["ls"]).code, 0);
    let claims = ClaimsOf::default();
    claims.set(&ids[1], Claim::Dispatching);
    let args: Vec<OsString> = ["rm", &ids[1], "att-synthetic-none"]
        .iter()
        .map(OsString::from)
        .collect();
    let refused = bench.cli_with(&args, T0, &Free::new(PLENTY), &claims);
    assert_eq!(refused.code, 14, "{refused:?}");

    // A store that can't be opened.
    fs::set_permissions(&bench.state, fs::Permissions::from_mode(0o750)).unwrap();
    for args in [vec!["add", paths[1]], vec!["ls"], vec!["rm", &ids[1]]] {
        let run = bench.cli(&args);
        assert_eq!(run.code, 10, "{args:?}: {run:?}");
        assert!(run.out.is_empty());
    }
    fs::set_permissions(&bench.state, fs::Permissions::from_mode(0o700)).unwrap();
}
