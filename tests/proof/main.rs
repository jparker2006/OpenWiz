//! The OpenWiz 0.0.1 proof cases, P1 to P12, written before the behaviour.
//!
//! Fixture tests show that the logic is right for synthetic inputs. They
//! never prove live behaviour; that needs live evidence (ticket 3).
//!
//! Only P8(a), the static allowlist, passes in ticket 1. Every other case is
//! marked `#[ignore = "pending: ticket 2"]`, and `tests/checks` keeps that
//! list exact. Run them with `cargo test -p openwiz-tests --test proof -- --ignored`.

mod support;

#[allow(dead_code)]
#[path = "../support/schema.rs"]
mod schema;

#[allow(dead_code)]
#[path = "../support/dirs.rs"]
mod dirs;

mod p01_coverage;
mod p02_freshness;
mod p03_unknowns;
mod p04_partial_failures;
mod p04b_source_diagnostics;
mod p05_paging_and_budget;
mod p06_verified_grouping;
mod p07_bounded_output;
mod p08a_allowlist_static;
mod p09_reported_configuration;
mod p10_json_matches_text;
mod p11_visibility;
mod p12_drift;
