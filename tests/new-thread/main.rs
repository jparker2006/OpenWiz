mod fake;
mod harness;
mod support;

#[allow(dead_code)]
#[path = "../support/schema.rs"]
mod schema;

#[allow(dead_code)]
#[path = "../support/dirs.rs"]
mod dirs;

mod boundary_readers;
mod contract_tables;
mod n01_end_to_end;
mod n01_exact_invocation;
mod n02_no_rewrite;
mod n03_existing_projects;
mod n04_offered_only;
mod n05_honest_options;
mod n06_outcome_mapping;
mod n07_requested_and_reported;
mod n08_start_stages;
mod n09_lost_acknowledgement;
mod n10_no_retry;
mod n11_op_id_binding;
mod n12_cross_process_claim;
mod n13_crash_recovery;
mod n14_prompt_seam;
mod n15_nothing_else_changes;
mod n16_allowlist;
mod n17_drift;
mod n18_cli;
mod n18_report;
mod n19_status_read_only;
mod pattern;
mod project_names;
mod record_store;
mod schema_parity;
mod structure;

// B2 (the implementation): attachments in new_thread (AD1, AD4, AD6, AD7, AD9,
// AD10, AD13).
#[path = "../attachments/staging.rs"]
mod staging;

mod ad_new_thread_attachments;
