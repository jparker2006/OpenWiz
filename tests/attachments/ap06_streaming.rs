//! AP6 Streaming. A file at its size limit is staged with bounded memory,
//! measured: the child process's peak resident memory (Linux's `VmHWM`)
//! grows by less than [`BOUND_KIB`] while it stages a 25 MiB file or a
//! 10 MiB image. Holding the file whole would add at least its size.

use std::fs::File;
use std::io::Write;

use openwiz_core::attachment::{FILE_MAX_BYTES, IMAGE_MAX_BYTES};
use openwiz_core::messaging::Sha256Hasher;
use serde_json::json;

use crate::bench::Bench;
use crate::child::{result, run_child};
use crate::support::PNG;

/// The most staging may add to the peak: a few copy buffers and the
/// metadata, far under the smaller limit (10 MiB).
const BOUND_KIB: u64 = 4 * 1024;

/// Writes `len` bytes, `head` first, in pieces, and returns their SHA-256.
fn write_large(path: &std::path::Path, head: &[u8], len: u64) -> String {
    let mut file = File::create(path).unwrap();
    let mut hasher = Sha256Hasher::new();
    file.write_all(head).unwrap();
    hasher.update(head);
    let piece: Vec<u8> = (0..65_536u32)
        .map(|i| b"ZZZZsynthetic"[i as usize % 13])
        .collect();
    let mut left = len - head.len() as u64;
    while left > 0 {
        let n = left.min(piece.len() as u64) as usize;
        file.write_all(&piece[..n]).unwrap();
        hasher.update(&piece[..n]);
        left -= n as u64;
    }
    hasher.finish()
}

#[test]
fn a_file_at_its_limit_is_staged_with_bounded_memory() {
    let bench = Bench::new("ap06_streaming");
    for (name, head, len) in [
        (
            "syn-ap06-large.txt",
            &b"synthetic text "[..],
            FILE_MAX_BYTES,
        ),
        ("syn-ap06-large.png", PNG, IMAGE_MAX_BYTES),
    ] {
        let source = bench.sources.join(name);
        let sha256 = write_large(&source, head, len);
        let out = bench.root.join(format!("{name}.json"));
        let child = run_child(&json!({
            "mode": "memory",
            "state": bench.state,
            "source": source,
            "out": out,
        }));
        assert!(child.status.success(), "{name}: {child:?}");
        let measured = result(&out);
        assert_eq!(measured["sha256"], sha256, "{name}: staged, with its bytes");
        let before = measured["before_kib"].as_u64().unwrap();
        let after = measured["after_kib"].as_u64().unwrap();
        assert!(
            after - before < BOUND_KIB,
            "{name}: staging {len} bytes raised the peak from {before} KiB to {after} KiB"
        );
    }
}
