//! OpenWiz adapter for CloudRoom: reads sessions through the existing,
//! authenticated `room-cli` and translates them for the core, and makes
//! OpenWiz's two writes: `thread tell`, for `send` ([`messaging`]), and
//! `thread spawn`, for `new` ([`new_thread`], OpenWiz 0.0.4), which also
//! reads the Cloud catalogue with `cloud status`.
//!
//! Every call runs with the CloudRoom thread-context variables removed from
//! its environment ([`is_scrubbed`]).
//!
//! Every read is one [`allowlist::Invocation`], run by `process` with a
//! 15-second timeout inside the run's deadline and no retry; [`source`]
//! translates the JSON of `room-cli` 0.43.1. Product rules (grouping,
//! visibility, paging, limits, completeness) are the core's.

pub mod allowlist;
pub mod messaging;
pub mod new_thread;
mod process;
pub mod source;

pub use process::{KEPT_ENV, SCRUBBED_ENV, SCRUBBED_ENV_PREFIX, is_scrubbed};

/// The program the adapter runs.
pub const PROGRAM: &str = "room-cli";

/// The `room-cli` version the live check ran against (0.0.1, 2026-10-29).
/// Reported, never a gate: any other version is marked untested.
pub const TESTED_VERSION: &str = "v127";
