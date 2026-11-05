//! The OpenWiz 0.0.3 messaging proof cases, M1 to M15, and
//! M16 to M18, each written
//! before the behaviour.
//!
//! Every case runs against the repository's fake `room-cli`. **Nothing is
//! sent live**, and fixture tests never prove live behaviour: the `thread
//! tell` shapes are documented, not observed, until ticket 3.
//!
//! `structure` holds the checks that are not one M case: dependency
//! direction for the new store crate, and a core without I/O.

mod support;

#[allow(dead_code)]
#[path = "../support/schema.rs"]
mod schema;

#[allow(dead_code)]
#[path = "../support/dirs.rs"]
mod dirs;

mod m01_exact_invocation;
mod m02_one_target;
mod m03_mode_rules;
mod m04_outcome_mapping;
mod m05_effects;
mod m06_lost_acknowledgement;
mod m07_op_id_binding;
mod m08_cross_process_claim;
mod m09_crash_recovery;
mod m10_record;
mod m11_inventory_unchanged;
mod m12_allowlist;
mod m13_drift;
mod m14_cli;
mod m15_text_only;
mod m16_preview;
mod m17_lookup;
mod m18_in_flight;
mod structure;

// B2: attachments in send (AD1 to AD5, AD7, AD9, AD10,
// AD13), and the memory measurement of `room-cli`'s memory on the fake.
#[path = "../attachments/staging.rs"]
mod staging;

mod ad_send_attachments;
mod b8_memory;
