//! Measure attachment dispatch memory with the fake room-cli.

use openwiz_core::messaging::Mode;
use openwiz_core::messaging::send::{self, Status};
use serde_json::json;

use crate::staging::{self, png, stage, uploads};
use crate::support::*;

const MIB: usize = 1024 * 1024;

#[cfg(target_os = "linux")]
#[test]
fn b8_the_fakes_peak_memory_grows_with_the_dispatchs_total_bytes() {
    let fake = SendFake::with(
        "b8_memory",
        vec![json!({
            "argv": ["thread", "tell", "thr_syn_0001", "--mode", "queue", "--json", "--"],
            "any_text": true,
            "any_attachments": true,
            "stdout_file": send_fixtures().join("tell-sent.json").to_str().unwrap(),
        })],
    );
    let sets: [(&str, Vec<usize>); 3] = [
        ("1 file of 1 MiB", vec![MIB]),
        ("1 file of 25 MiB", vec![25 * MIB]),
        ("4 files of 25 MiB", vec![25 * MIB; 4]),
    ];
    let mut measured = Vec::new();
    for (i, (label, sizes)) in sets.iter().enumerate() {
        let ids = sizes
            .iter()
            .enumerate()
            .map(|(j, size)| {
                let mut bytes = vec![0x5a_u8; *size];
                bytes[..9].copy_from_slice(b"Synthetic");
                stage(&fake.state, &format!("syn-b8-{i}-{j}.bin"), &bytes, T0)
                    .attachment
                    .id
            })
            .collect();
        let mut r = request(
            "thr_syn_0001",
            Mode::Queue,
            "Synthetic",
            &format!("op-b8-{i}"),
        );
        r.prompt.attachments = ids;
        r.budget = std::time::Duration::from_secs(30);
        let report = send::send_with(
            &fake.source(),
            &fake.store(),
            &staging::store(&fake.state),
            &clock(),
            &r,
        )
        .unwrap();
        assert_eq!(report.status, Status::Accepted, "{label}");
        let line = uploads(&fake.dir).pop().unwrap();
        let peak_kib = line["vm_hwm_kib"].as_u64().expect("VmHWM on Linux");
        let total: usize = sizes.iter().sum();
        // The model holds every byte at once: its peak is at least the total.
        assert!(peak_kib * 1024 >= total as u64, "{label}: {peak_kib} KiB");
        measured.push((label, total / MIB, peak_kib / 1024));
    }
    // Linear in the total: the 100 MiB set's peak is above the 25 MiB one's
    // by at least the 75 MiB between them.
    assert!(measured[2].2 >= measured[1].2 + 75, "{measured:?}");
    for (label, total, peak) in &measured {
        println!("b8: {label}: {total} MiB attached, fake peak {peak} MiB");
    }
    let _ = png;
}
