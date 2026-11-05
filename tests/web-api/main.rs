#[path = "../proof/support.rs"]
mod fake;

#[allow(dead_code)]
#[path = "../support/schema.rs"]
mod schema;

#[allow(dead_code)]
#[path = "../support/dirs.rs"]
mod dirs;

mod http;

#[path = "../messaging/support.rs"]
mod msg;

mod send_support;

mod new_support;

mod a3_loopback;
mod a4_no_mutation;
mod a5_bounds;
mod a6_semantics;
mod a7_headers_and_logs;
mod a7_source_diagnostics;
mod a8_web_page;
mod contract;
mod w15_no_redispatch;
mod w1_one_mutation;
mod w2_owner;
mod w3_cross_site;
mod w4_bounds;
mod w5_same_operation;
mod w6_never_coalesced;

mod u01_new_routes;
mod u02_u03_owner_and_cross_site;
mod u04_bounds;
mod u05_same_operation;
mod u06_u12_never_coalesced;
mod u09_u11_preview_and_status;

#[path = "../attachments/staging.rs"]
mod staging;

mod attach_support;

mod aw01_routes;
mod aw02_owner_and_cross_site;
mod aw03_bounds;
mod aw03_streaming;
mod aw04_same_operation;
mod aw05_gate;
