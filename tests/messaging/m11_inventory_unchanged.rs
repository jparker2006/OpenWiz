//! M11 The inventory is unchanged: P1 to P12 still pass (the `proof` test
//! binary, unchanged apart from P8(a) for M12), the inventory API keeps its
//! signatures, and no inventory path can reach the write.

use std::fs;
use std::path::Path;

use openwiz_adapter_cloudroom::source::CloudRoomSource;
use openwiz_core::envelope::Envelope;
use openwiz_core::model::{Inventory, SessionView};
use openwiz_core::ops::{self, Clock, ListRequest, OpError, Report, ShowRequest};
use openwiz_core::source::SessionSource;

use crate::support::*;

/// The inventory operations' signatures, as the implementation's server calls them.
type ListOp<T> = fn(&dyn SessionSource, &dyn Clock, &ListRequest) -> Result<T, OpError>;
type ShowOp<T> = fn(&dyn SessionSource, &dyn Clock, &ShowRequest) -> Result<T, OpError>;

#[test]
fn the_inventory_api_keeps_its_signatures() {
    // A changed signature fails to compile.
    let _: ListOp<Report<Inventory>> = ops::list_report;
    let _: ShowOp<Report<SessionView>> = ops::show_report;
    let _: ListOp<Envelope<Inventory>> = ops::list;
    let _: ShowOp<Envelope<SessionView>> = ops::show;
    let _: fn(String) -> CloudRoomSource = |program| CloudRoomSource::new(program);
    let _: fn() -> CloudRoomSource = CloudRoomSource::from_path;
}

#[test]
fn no_inventory_command_reaches_the_write() {
    let fake = SendFake::new("m11_inventory", "send");
    let source = fake.source();
    let ids = [
        "thr_syn_0001",
        "thr_syn_0002",
        "thr_syn_0003",
        "thr_syn_0004",
        "thr_syn_0005",
        "thr_syn_0006",
        "thr_syn_0007",
        "thr_syn_0008",
        "thr_syn_0011",
        "thr_syn_0012",
        "thr_syn_9999",
    ];
    let mut commands: Vec<Vec<&str>> = vec![
        vec!["ls"],
        vec!["ls", "-l"],
        vec!["ls", "-l", "-a", "--hidden", "--json"],
        vec!["ls", "--limit", "1", "--offset", "2"],
        vec!["--help"],
    ];
    for id in &ids {
        commands.push(vec!["show", id]);
        commands.push(vec!["show", id, "--json"]);
    }
    for args in commands {
        let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let _ = openwiz_cli::run(&args, &source, &clock(), &mut out, &mut err);
    }
    // `send` is not an inventory command: `run` refuses it.
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = openwiz_cli::run(
        &[
            "send".to_owned(),
            "thr_syn_0001".to_owned(),
            "Synthetic".to_owned(),
        ],
        &source,
        &clock(),
        &mut out,
        &mut err,
    );
    assert_eq!(code, 2, "{}", String::from_utf8_lossy(&err));
    assert!(fake.invocations().len() > 20, "the inventory read");
    assert!(fake.tells().is_empty(), "{:?}", fake.tells());
    assert_eq!(fake.tell_count(), 0);
}

#[test]
fn the_inventory_code_cannot_name_the_write() {
    for file in [
        "crates/core/src/ops.rs",
        "crates/core/src/source.rs",
        "crates/core/src/envelope.rs",
        "crates/core/src/model.rs",
        "crates/adapters/cloudroom/src/source.rs",
        "crates/adapters/cloudroom/src/process.rs",
    ] {
        let text = fs::read_to_string(Path::new(ROOT).join(file)).unwrap();
        for word in ["ThreadTell", "messaging", "Messenger", "tell("] {
            assert!(!text.contains(word), "{file} names {word}");
        }
    }
    let source = fs::read_to_string(Path::new(ROOT).join("crates/core/src/source.rs")).unwrap();
    let trait_body = source
        .split("pub trait SessionSource")
        .nth(1)
        .expect("the trait");
    assert!(
        !trait_body.contains("fn send") && !trait_body.contains("fn tell"),
        "the inventory interface has no write"
    );
}
