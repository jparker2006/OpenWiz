//! AP7 Low disk. Staging is refused, with "low disk", when it would leave
//! less than 1 GiB free on the state volume (sheet the check), before anything is
//! written: no `.partial` and no directory are left. Free space that can't
//! be measured stages nothing either (fail closed).

use std::ffi::OsString;

use openwiz_core::attachment::{FREE_DISK_FLOOR_BYTES, FailReason};
use openwiz_store::attachments::{NoClaims, Staged};

use crate::bench::{Bench, Free, T0, Unmeasurable, walk};
use crate::support::{PNG, padded};

#[test]
fn below_the_floor_nothing_is_written() {
    let bench = Bench::new("ap07_floor");
    let bytes = padded(PNG, 50_000);
    let size = bytes.len() as u64;
    let source = bench.source("syn-ap07-photo.png", &bytes);
    let store = bench.store();

    let short = Free::new(size + FREE_DISK_FLOOR_BYTES - 1);
    assert_eq!(
        store.stage(&source, T0, &short).unwrap(),
        Staged::Failed(FailReason::LowDisk)
    );
    assert_eq!(
        *short.asked.lock().unwrap(),
        vec![bench.attachments()],
        "the state volume is measured, where the copy would go"
    );
    let left = walk(&bench.state);
    assert!(
        left.iter().all(|p| !p.ends_with(".partial")),
        "a partial is left: {left:?}"
    );
    assert!(bench.ids().is_empty(), "a directory is left: {left:?}");

    // Exactly at the floor, it stages.
    let enough = Free::new(size + FREE_DISK_FLOOR_BYTES);
    assert!(matches!(
        store.stage(&source, T0, &enough).unwrap(),
        Staged::Ready(_)
    ));
}

#[test]
fn the_cli_says_low_disk_and_exits_5() {
    let bench = Bench::new("ap07_cli");
    let source = bench.source("syn-ap07-cli.png", &padded(PNG, 1_000));
    let args = [OsString::from("add"), source.clone().into_os_string()];
    let run = bench.cli_with(&args, T0, &Free::new(FREE_DISK_FLOOR_BYTES), &NoClaims);
    assert_eq!(run.code, 5, "{run:?}");
    assert!(run.out.contains("failed (low_disk)"), "{}", run.out);
    assert!(run.out.contains("less than 1 GiB free"), "{}", run.out);
    assert!(bench.ids().is_empty());
}

#[test]
fn free_space_that_cant_be_measured_stages_nothing() {
    let bench = Bench::new("ap07_unmeasurable");
    let source = bench.source("syn-ap07-unmeasured.png", &padded(PNG, 1_000));
    let store = bench.store();
    let error = store.stage(&source, T0, &Unmeasurable).unwrap_err();
    assert!(error.0.contains("free space"), "{error:?}");
    assert!(bench.ids().is_empty());

    let args = [OsString::from("add"), source.into_os_string()];
    let run = bench.cli_with(&args, T0, &Unmeasurable, &NoClaims);
    assert_eq!(run.code, 10, "{run:?}");
    assert!(run.out.contains("not staged"), "{}", run.out);
    assert!(bench.ids().is_empty());
}

/// One `df -P -k` output and what `parse_df` must make of it: the bytes
/// available, or a refusal (`None`).
const DF_OUTPUTS: &[(&str, &str, Option<u64>)] = &[
    (
        "GNU coreutils",
        "Filesystem     1024-blocks     Used Available Capacity Mounted on\n\
         /dev/syn0p1       50620216 10256464  40347368      21% /\n",
        Some(40_347_368 * 1024),
    ),
    (
        "macOS",
        "Filesystem   1024-blocks      Used Available Capacity  Mounted on\n\
         /dev/disk9s9   971350180 402412340 524116808    44%    /System/Volumes/Data\n",
        Some(524_116_808 * 1024),
    ),
    (
        "a full volume",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
         /dev/syn1 100 100 0 100% /syn\n",
        Some(0),
    ),
    (
        "a volume named by digits",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
         123 1000 500 400 56% /syn\n",
        Some(400 * 1024),
    ),
    (
        "no last line break",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
         /dev/syn2 9 1 8 12% /",
        Some(8 * 1024),
    ),
    ("nothing", "", None),
    (
        "counters only ",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
         1 2 999999999 100%\n",
        None,
    ),
    (
        "no mount point ",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
         /dev/syn 1 2 999999999 100%\n",
        None,
    ),
    (
        "counters inside the name, the real ones 0 ",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
         map 1 2 999999999 100% home 100 100 0 100% /home\n",
        None,
    ),
    (
        "a mount point with spaces",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
         /dev/syn13 100 50 50 50% /mnt/synthetic disk\n",
        None,
    ),
    (
        "a mount point with counters in it",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
         /dev/syn14 100 100 0 100% /mnt/a 7 8 999999 10%\n",
        None,
    ),
    (
        "a volume name with spaces",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
         map syn home 0 0 0 100% /home\n",
        None,
    ),
    (
        "a relative mount point",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
         /dev/syn15 9 1 8 12% syn\n",
        None,
    ),
    (
        "seven fields",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
         /dev/syn16 9 1 8 12% / extra\n",
        None,
    ),
    (
        "the header only",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n",
        None,
    ),
    (
        "two volumes",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
         /dev/syn3 9 1 8 12% /\n/dev/syn4 9 1 8 12% /x\n",
        None,
    ),
    (
        "512-byte blocks (no -k)",
        "Filesystem 512-blocks Used Available Capacity Mounted on\n\
         /dev/syn5 18 2 16 12% /\n",
        None,
    ),
    (
        "a localized header",
        "Dateisystem 1K-Blöcke Benutzt Verfügbar Verw% Eingehängt auf\n\
         /dev/syn6 9 1 8 12% /\n",
        None,
    ),
    (
        "no capacity",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
         /dev/syn7 9 1 8 - /\n",
        None,
    ),
    (
        "a fraction",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
         /dev/syn8 9 1 8.5 12% /\n",
        None,
    ),
    (
        "a negative number",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
         /dev/syn9 9 1 -8 12% /\n",
        None,
    ),
    (
        "only two numbers",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
         /dev/syn10 9 8 12% /\n",
        None,
    ),
    (
        "too large to count in bytes",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
         /dev/syn11 1 1 18014398509481984 12% /\n",
        None,
    ),
    (
        "too large for a number",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
         /dev/syn12 1 1 99999999999999999999999 12% /\n",
        None,
    ),
];

#[test]
fn dfs_output_is_read_only_in_its_posix_form() {
    use openwiz_store::attachments::parse_df;
    for (case, output, expected) in DF_OUTPUTS {
        assert_eq!(parse_df(output.as_bytes()).ok(), *expected, "{case}");
    }
    assert!(
        parse_df(b"Filesystem 1024-blocks Used Available Capacity Mounted on\n\xff 9 1 8 12% /\n")
            .is_err()
    );
}

#[test]
fn the_real_measure_is_bin_df_and_fails_closed() {
    use openwiz_store::attachments::{DF_ARGS, DF_PROGRAM, Df, FreeSpace, parse_df};
    assert_eq!(DF_PROGRAM, "/bin/df");
    assert_eq!(DF_ARGS, ["-P", "-k"]);
    let bench = Bench::new("ap07_real_measure");
    let store = bench.store();
    let free = Df
        .free_bytes(&bench.attachments())
        .expect("/bin/df measures the state volume");
    // The tests' own run of the same command, parsed the same way, agrees
    // within 1 GiB (other processes write meanwhile).
    let out = std::process::Command::new(DF_PROGRAM)
        .args(DF_ARGS)
        .arg(bench.attachments())
        .env_clear()
        .env("LC_ALL", "C")
        .output()
        .unwrap();
    assert!(out.status.success());
    let theirs = parse_df(&out.stdout).unwrap();
    assert!(
        free.abs_diff(theirs) < FREE_DISK_FLOOR_BYTES,
        "{free} vs {theirs}"
    );
    // A directory that isn't there: df fails, so nothing is measured.
    assert!(Df.free_bytes(&bench.root.join("syn-ap07-none")).is_err());
    // Staging through the real measure.
    let source = bench.source("syn-ap07-real.png", &padded(PNG, 1_000));
    assert!(matches!(
        store.stage(&source, T0, &Df).unwrap(),
        Staged::Ready(_)
    ));
}
