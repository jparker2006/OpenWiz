//! Foundation checks: dependency direction, the
//! fixture scan, the approved limits, schema conformance and the pending list.

#[path = "../support/schema.rs"]
mod schema;

mod dependencies;
mod fixture_scan;
mod limits;
mod pending;
mod schema_conformance;
