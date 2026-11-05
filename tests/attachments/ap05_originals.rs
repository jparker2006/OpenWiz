//! AP5 Originals unchanged. The hash, size, mtime and mode of every source
//! file are the same after `add`, nothing is written beside them, and each
//! is opened for reading only (a read-only source stages). A symlink is
//! followed only to a regular file: folders, links to folders, dangling
//! links, FIFOs and missing paths are refused, and so is a FIFO swapped in
//! between the look and the open, without waiting for a writer.

use std::fs::{self, OpenOptions};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

use openwiz_core::attachment::FailReason;
use openwiz_store::attachments::{Durability, Staged, Step};

use crate::bench::{Bench, CONTENT, Free, PLENTY, T0, sha256_of, walk};
use crate::support::{JPEG, PNG, SVG, ZIP, padded};

/// What must not change about a source: its hash, size, mtime and mode,
/// and for a link the link itself too.
fn fingerprint(path: &Path) -> (String, u64, SystemTime, u32, Option<SystemTime>) {
    let meta = fs::metadata(path).unwrap();
    let link = fs::symlink_metadata(path).unwrap();
    (
        sha256_of(path),
        meta.len(),
        meta.modified().unwrap(),
        meta.permissions().mode(),
        link.file_type()
            .is_symlink()
            .then(|| link.modified().unwrap()),
    )
}

#[test]
fn every_source_is_unchanged_and_only_read() {
    let bench = Bench::new("ap05_unchanged");
    let png = bench.source("syn-ap05-photo.png", &padded(PNG, 70_000));
    let jpeg = bench.source("syn-ap05-photo.jpg", &padded(JPEG, 2_000));
    let text = bench.source("syn-ap05-notes.txt", CONTENT.as_bytes());
    let zip = bench.source("syn-ap05-bundle.zip", &padded(ZIP, 3_000));
    let svg = bench.source("syn-ap05-drawing.svg", SVG);
    // Read-only: opening it for writing fails, so a read-write open would.
    let read_only = bench.source("syn-ap05-read-only.txt", CONTENT.as_bytes());
    fs::set_permissions(&read_only, fs::Permissions::from_mode(0o400)).unwrap();
    assert!(
        OpenOptions::new().write(true).open(&read_only).is_err(),
        "AP5 needs a user that can't write a read-only file (not root): run the tests as an \
         ordinary user"
    );
    let nested = bench.sources.join("syn-ap05-folder");
    crate::dirs::private_dir(&nested).unwrap();
    let inner = nested.join("syn-ap05-inner.txt");
    fs::write(&inner, CONTENT).unwrap();
    let link = bench.sources.join("syn-ap05-link.txt");
    std::os::unix::fs::symlink(&inner, &link).unwrap();

    let sources = [&png, &jpeg, &text, &zip, &svg, &read_only, &inner, &link];
    let before: Vec<_> = sources.iter().map(|p| fingerprint(p)).collect();
    let listing = walk(&bench.sources);

    let args: Vec<&str> = ["add", "--json"]
        .into_iter()
        .chain(sources.iter().map(|p| p.to_str().unwrap()))
        .collect();
    let run = bench.cli(&args);
    assert_eq!(run.code, 0, "{run:?}");

    let after: Vec<_> = sources.iter().map(|p| fingerprint(p)).collect();
    assert_eq!(after, before, "a source changed");
    assert_eq!(
        walk(&bench.sources),
        listing,
        "something was written beside a source"
    );

    // Each copy is the source's bytes exactly.
    let json = run.json();
    let ready = json["ready"].as_array().unwrap();
    assert_eq!(ready.len(), sources.len());
    for (entry, source) in ready.iter().zip(sources) {
        let attachment = &entry["staged"]["attachment"];
        let copy = bench
            .attachments()
            .join(attachment["id"].as_str().unwrap())
            .join(attachment["name"].as_str().unwrap());
        assert_eq!(sha256_of(&copy), sha256_of(source));
        assert_eq!(attachment["sha256"], sha256_of(source));
    }
    // The link's copy has the link's own name, not its target's.
    assert_eq!(
        ready[7]["staged"]["attachment"]["name"],
        "syn-ap05-link.txt"
    );
}

#[test]
fn folders_links_to_folders_and_other_non_files_are_refused() {
    let bench = Bench::new("ap05_refused");
    let folder = bench.sources.join("syn-ap05-a-folder");
    crate::dirs::private_dir(&folder).unwrap();
    fs::write(folder.join("syn-ap05-inside.txt"), CONTENT).unwrap();
    let to_folder = bench.sources.join("syn-ap05-to-folder");
    std::os::unix::fs::symlink(&folder, &to_folder).unwrap();
    let dangling = bench.sources.join("syn-ap05-dangling.txt");
    std::os::unix::fs::symlink(bench.sources.join("syn-ap05-nothing"), &dangling).unwrap();
    let fifo = bench.sources.join("syn-ap05-fifo");
    mkfifo(&fifo);
    let to_fifo = bench.sources.join("syn-ap05-to-fifo.txt");
    std::os::unix::fs::symlink(&fifo, &to_fifo).unwrap();
    let missing = bench.sources.join("syn-ap05-missing.txt");
    let listing = walk(&bench.sources);

    let cases = [
        (&folder, "folder"),
        (&to_folder, "folder"),
        (&dangling, "unreadable"),
        (&fifo, "unreadable"),
        (&to_fifo, "unreadable"),
        (&missing, "unreadable"),
    ];
    let paths: Vec<&str> = cases.iter().map(|(p, _)| p.to_str().unwrap()).collect();
    let text = bench.cli(&[&["add"][..], &paths].concat());
    assert_eq!(text.code, 5, "{text:?}");
    assert!(
        text.out.contains("Folders aren't supported yet"),
        "{}",
        text.out
    );
    let run = bench.cli(&[&["add", "--json"][..], &paths].concat());
    assert_eq!(run.code, 5, "{run:?}");
    let json = run.json();
    assert!(json["ready"].as_array().unwrap().is_empty());
    let failed = json["failed"].as_array().unwrap();
    assert_eq!(failed.len(), cases.len());
    for (i, (entry, (path, reason))) in failed.iter().zip(cases).enumerate() {
        assert_eq!(entry["index"], i, "{entry}");
        assert_eq!(entry["path"], path.to_str().unwrap());
        assert_eq!(entry["readiness"]["state"], "failed");
        assert_eq!(entry["readiness"]["reason"], reason, "{path:?}");
    }
    assert_eq!(walk(&bench.sources), listing);
    assert!(bench.ids().is_empty(), "nothing was staged");
}

fn mkfifo(path: &Path) {
    let made = Command::new("mkfifo").arg(path).status().expect("mkfifo");
    assert!(made.success(), "mkfifo failed");
}

/// Swaps a FIFO in for the source once it has been looked at, before it is
/// opened.
struct SwapInFifo(PathBuf);

impl Durability for SwapInFifo {
    fn done(&self, step: Step) {
        if step == Step::Looked {
            fs::remove_file(&self.0).unwrap();
            mkfifo(&self.0);
        }
    }
}

#[test]
fn a_fifo_swapped_in_after_the_look_is_refused_without_hanging() {
    let bench = Bench::new("ap05_swapped_fifo");
    let source = bench.source("syn-ap05-swapped.txt", CONTENT.as_bytes());
    let store = bench.store();
    let (sent, received) = mpsc::channel();
    let path = source.clone();
    std::thread::spawn(move || {
        let staged = store.stage_with(&path, T0, &Free::new(PLENTY), &SwapInFifo(path.clone()));
        let _ = sent.send(staged);
    });
    // No writer ever opens the FIFO: a blocking open would wait for ever.
    let staged = received
        .recv_timeout(Duration::from_secs(20))
        .expect("staging waited on a FIFO swapped in after the look");
    assert_eq!(staged.unwrap(), Staged::Failed(FailReason::Unreadable));
    assert!(
        fs::symlink_metadata(&source).unwrap().file_type().is_fifo(),
        "the FIFO is still there, untouched"
    );
    assert!(bench.ids().is_empty(), "nothing was staged");
}

/// What [`SwapAfterLook`] puts at the source's path once it has been looked
/// at, before it is opened.
enum Swap {
    /// A link to another regular file.
    LinkTo(PathBuf),
    /// Another regular file under the same name (the original moved away,
    /// so its inode can't be reused).
    Replace(Vec<u8>),
}

struct SwapAfterLook(PathBuf, Swap);

impl Durability for SwapAfterLook {
    fn done(&self, step: Step) {
        if step != Step::Looked {
            return;
        }
        let moved = self.0.with_extension("moved");
        fs::rename(&self.0, &moved).unwrap();
        match &self.1 {
            Swap::LinkTo(other) => std::os::unix::fs::symlink(other, &self.0).unwrap(),
            Swap::Replace(bytes) => fs::write(&self.0, bytes).unwrap(),
        }
    }
}

#[test]
fn a_file_swapped_in_after_the_look_is_refused() {
    let bench = Bench::new("ap05_swapped_file");
    let other = bench.source("syn-ap05-other.txt", b"SYNTHETIC-CONTENT, swapped in\n");
    let target = bench.source("syn-ap05-target.txt", CONTENT.as_bytes());
    let store = bench.store();
    for (case, link, swap) in [
        ("a link to another file", false, Swap::LinkTo(other.clone())),
        (
            "another file",
            false,
            Swap::Replace(b"SYNTHETIC-CONTENT, new\n".to_vec()),
        ),
        (
            "a chosen link, pointed elsewhere",
            true,
            Swap::LinkTo(other.clone()),
        ),
    ] {
        let source = bench
            .sources
            .join(format!("syn-ap05-swap-{}.txt", case.len()));
        if link {
            std::os::unix::fs::symlink(&target, &source).unwrap();
        } else {
            fs::write(&source, CONTENT).unwrap();
        }
        let staged = store
            .stage_with(
                &source,
                T0,
                &Free::new(PLENTY),
                &SwapAfterLook(source.clone(), swap),
            )
            .unwrap();
        assert_eq!(staged, Staged::Failed(FailReason::SizeMismatch), "{case}");
        assert!(bench.ids().is_empty(), "{case}: nothing was staged");
    }
    // Unswapped, a chosen link stages its target's bytes.
    let link = bench.sources.join("syn-ap05-link-kept.txt");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let Staged::Ready(ready) = store.stage(&link, T0, &Free::new(PLENTY)).unwrap() else {
        panic!("a chosen link to a regular file stages");
    };
    assert_eq!(ready.attachment.sha256.as_str(), sha256_of(&target));
    assert_eq!(
        fs::read(&other).unwrap(),
        b"SYNTHETIC-CONTENT, swapped in\n",
        "the file swapped in is untouched"
    );
}
