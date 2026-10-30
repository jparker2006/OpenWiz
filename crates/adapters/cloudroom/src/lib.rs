//! OpenWiz adapter for CloudRoom: reads sessions through the existing,
//! authenticated `room-cli` and translates them for the core.
//!
//! Every read is one [`allowlist::Invocation`], run by `process` with a
//! 15-second timeout inside the run's deadline and no retry; [`source`]
//! translates the JSON of `room-cli` 0.43.1. Product rules (grouping,
//! visibility, paging, limits, completeness) are the core's.

pub mod allowlist;
mod process;
pub mod source;

/// The program the adapter runs.
pub const PROGRAM: &str = "room-cli";

/// The `room-cli` version the live check ran against (0.0.1, 2026-10-29).
/// Reported, never a gate: any other version is marked untested.
pub const TESTED_VERSION: &str = "v127";
