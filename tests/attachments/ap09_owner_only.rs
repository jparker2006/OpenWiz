use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use openwiz_store::AttachmentStore;
use openwiz_store::attachments::{CLAIMS_LOCK, META, STAGING_LOCK};

use crate::bench::{Bench, CONTENT, mode, walk};
use crate::support::ROOT;

#[test]
fn every_directory_and_file_is_the_owners_only() {
    let bench = Bench::new("ap09_modes");
    let source = bench.source("syn-ap09-notes.txt", CONTENT.as_bytes());
    let run = bench.cli(&["add", "--json", source.to_str().unwrap()]);
    assert_eq!(run.code, 0, "{run:?}");
    let id = run.json()["ready"][0]["staged"]["attachment"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let attachments = bench.attachments();
    let dir = attachments.join(&id);
    assert_eq!(mode(&bench.state), 0o700);
    assert_eq!(mode(&attachments), 0o700);
    assert_eq!(mode(&attachments.join(STAGING_LOCK)), 0o600);
    // B2 (the implementation): the claims lock, which every sweep holds while it
    // applies the retention rule, is the owner's only too.
    assert_eq!(mode(&attachments.join(CLAIMS_LOCK)), 0o600);
    assert_eq!(mode(&dir), 0o700);
    assert_eq!(mode(&dir.join("syn-ap09-notes.txt")), 0o400);
    assert_eq!(mode(&dir.join(META)), 0o600);
    // Nothing else: no log, no temporary file.
    assert_eq!(
        walk(&bench.state),
        [
            "attachments".to_owned(),
            format!("attachments/{CLAIMS_LOCK}"),
            format!("attachments/{STAGING_LOCK}"),
            format!("attachments/{id}"),
            format!("attachments/{id}/{META}"),
            format!("attachments/{id}/syn-ap09-notes.txt"),
        ]
    );
}

#[test]
fn a_state_directory_others_can_use_is_refused() {
    let bench = Bench::new("ap09_open_to_others");
    crate::dirs::private_dir_all(&bench.state).unwrap();
    fs::set_permissions(&bench.state, fs::Permissions::from_mode(0o755)).unwrap();
    let source = bench.source("syn-ap09-refused.txt", CONTENT.as_bytes());
    let run = bench.cli(&["add", source.to_str().unwrap()]);
    assert_eq!(run.code, 10, "{run:?}");
    assert!(run.out.is_empty(), "{}", run.out);
    assert_eq!(run.err.lines().count(), 1, "{}", run.err);
    assert!(run.err.starts_with("openwiz: "), "{}", run.err);
    assert!(run.err.contains("owner only"), "{}", run.err);
    assert!(walk(&bench.state).is_empty(), "nothing was made");
    let ls = bench.cli(&["ls"]);
    assert_eq!(ls.code, 10, "{ls:?}");
}

#[test]
fn a_link_in_place_of_attachments_is_refused() {
    let bench = Bench::new("ap09_link");
    crate::dirs::private_dir_all(&bench.state).unwrap();
    fs::set_permissions(&bench.state, fs::Permissions::from_mode(0o700)).unwrap();
    let elsewhere = bench.root.join("elsewhere");
    crate::dirs::private_dir(&elsewhere).unwrap();
    fs::set_permissions(&elsewhere, fs::Permissions::from_mode(0o700)).unwrap();
    std::os::unix::fs::symlink(&elsewhere, bench.attachments()).unwrap();
    assert!(AttachmentStore::open(&bench.state).is_err());
    let source = bench.source("syn-ap09-link.txt", CONTENT.as_bytes());
    let run = bench.cli(&["add", source.to_str().unwrap()]);
    assert_eq!(run.code, 10, "{run:?}");
    assert!(
        walk(&elsewhere).is_empty(),
        "nothing was written through the link"
    );
}

#[test]
fn nothing_is_written_inside_a_repository() {
    let bench = Bench::new("ap09_repository");
    let repo = bench.root.join("repo");
    crate::dirs::private_dir_all(repo.join(".git")).unwrap();
    let state = repo.join("state");
    assert!(AttachmentStore::open(&state).is_err());
    assert!(!state.exists(), "nothing was made inside the repository");
    // This repository: the tests' state directories are all outside it.
    let root = Path::new(ROOT).canonicalize().unwrap();
    assert!(
        !crate::bench::run_root()
            .canonicalize()
            .unwrap()
            .starts_with(&root)
    );
    assert!(!root.join("attachments").exists());
}
