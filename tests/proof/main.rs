//! The OpenWiz 0.0.1 proof cases, P1 to P12, written before the behaviour.
//!
//! Fixture tests show that the logic is right for synthetic inputs. They
//! never prove live behaviour; that needs live evidence.
//!
//! A case for behaviour not built yet is marked
//! `#[ignore = "pending: ticket 2"]`, and `tests/checks` keeps that list
//! exact. In 0.0.1 none is pending: every case runs.

mod support;

#[allow(dead_code)]
#[path = "../support/schema.rs"]
mod schema;

mod p01_coverage;
mod p02_freshness;
mod p03_unknowns;
mod p04_partial_failures;
mod p05_paging_and_budget;
mod p06_verified_grouping;
mod p07_bounded_output;
mod p08a_allowlist_static;
mod p09_reported_configuration;
mod p10_json_matches_text;
mod p11_visibility;
mod p12_drift;
