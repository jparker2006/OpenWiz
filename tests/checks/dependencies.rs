//! Dependency direction and the approved dependency set, from `cargo metadata`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

const CORE: &str = "openwiz-core";
const ADAPTER: &str = "openwiz-adapter-cloudroom";
const CLI: &str = "openwiz-cli";
const APPROVED_DIRECT: &[&str] = &["serde", "serde_json"];

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
}

fn metadata() -> Value {
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--locked", "--offline"])
        .current_dir(root())
        .output()
        .expect("run cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("metadata is JSON")
}

/// Package name to the names it depends on (normal dependencies only),
/// for the resolved graph.
fn graph(meta: &Value) -> BTreeMap<String, BTreeSet<String>> {
    let names: BTreeMap<&str, &str> = meta["packages"]
        .as_array()
        .expect("packages")
        .iter()
        .map(|p| (p["id"].as_str().unwrap(), p["name"].as_str().unwrap()))
        .collect();
    meta["resolve"]["nodes"]
        .as_array()
        .expect("resolve nodes")
        .iter()
        .map(|node| {
            let name = names[node["id"].as_str().unwrap()].to_owned();
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
            (name, deps)
        })
        .collect()
}

fn reachable(graph: &BTreeMap<String, BTreeSet<String>>, from: &str) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![from.to_owned()];
    while let Some(next) = stack.pop() {
        for dep in graph.get(&next).into_iter().flatten() {
            if seen.insert(dep.clone()) {
                stack.push(dep.clone());
            }
        }
    }
    seen
}

#[test]
fn the_core_depends_on_neither_the_adapter_nor_the_cli() {
    let g = graph(&metadata());
    let core = reachable(&g, CORE);
    assert!(
        !core.contains(ADAPTER),
        "core reaches the adapter: {core:?}"
    );
    assert!(!core.contains(CLI), "core reaches the CLI: {core:?}");
}

#[test]
fn dependencies_point_inward() {
    let g = graph(&metadata());
    let adapter = reachable(&g, ADAPTER);
    assert!(
        adapter.contains(CORE),
        "the adapter implements the core's interfaces"
    );
    assert!(
        !adapter.contains(CLI),
        "the adapter must not depend on a client"
    );
    let cli = reachable(&g, CLI);
    assert!(cli.contains(CORE) && cli.contains(ADAPTER), "{cli:?}");
}

#[test]
fn serde_and_serde_json_are_the_only_direct_dependencies() {
    let meta = metadata();
    let members: BTreeSet<&str> = meta["workspace_members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m.as_str().unwrap())
        .collect();
    for package in meta["packages"].as_array().unwrap() {
        if !members.contains(package["id"].as_str().unwrap()) {
            continue;
        }
        for dep in package["dependencies"].as_array().unwrap() {
            let name = dep["name"].as_str().unwrap();
            let is_workspace_crate = dep["path"].is_string();
            assert!(
                is_workspace_crate || APPROVED_DIRECT.contains(&name),
                "{} depends on {name}, which is not approved",
                package["name"]
            );
        }
    }
}

#[test]
fn dependencies_md_records_every_locked_crate_with_its_checksum() {
    let lock = fs::read_to_string(root().join("Cargo.lock")).expect("Cargo.lock");
    let record =
        fs::read_to_string(root().join("docs/dependencies.md")).expect("docs/dependencies.md");
    let mut locked = 0;
    for block in lock.split("[[package]]").skip(1) {
        let field = |key: &str| {
            block
                .lines()
                .find_map(|l| l.strip_prefix(&format!("{key} = \"")))
                .map(|v| v.trim_end_matches('"').to_owned())
        };
        let Some(checksum) = field("checksum") else {
            continue; // a workspace crate
        };
        let (name, version) = (field("name").unwrap(), field("version").unwrap());
        let row = record
            .lines()
            .find(|l| l.starts_with(&format!("| `{name}` |")))
            .unwrap_or_else(|| panic!("{name} is not in dependencies.md"));
        assert!(
            row.contains(&format!("`{version}`")),
            "{name}: version {version} not recorded"
        );
        assert!(row.contains(&checksum), "{name}: checksum not recorded");
        locked += 1;
    }
    assert!(locked >= 2, "the lock file has the approved crates");
}
