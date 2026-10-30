//! OpenWiz 0.0.2: proof tests for `openwiz-server`, written before the
//! server. See `contracts/web-api.v1.md`.
//!
//! - A3 the server listens on loopback only and refuses foreign `Host`s;
//! - A4 only `GET` on the three allowlisted routes, and only the core's
//!   list and show, with the same reads as the CLI;
//! - A5 queries, heads, bodies, connections and core runs are bounded,
//!   identical requests share one run, and slow clients time out;
//! - A6 every body equals `openwiz … --json` for the same options;
//! - A7 security headers, no CORS, no cache, and nothing leaked in errors
//!   or logs, including no `room-cli` diagnostic text;
//!
//! - A8 the web page, from one `--web-root` directory: an allowlist that
//!   matches the export, contained, `GET` and `HEAD` only, page headers
//!   (W1 to W11 in `a8_web_page.rs`).
//!
//! A1 and A2, and a performance baseline, are not fixture tests: they need
//! the real access path or the real `room-cli` (see `docs/evidence/0.0.2-live-check.md`).
//! Fixture tests never prove live behaviour.

#[path = "../proof/support.rs"]
mod fake;

#[allow(dead_code)]
#[path = "../support/schema.rs"]
mod schema;

mod http;

mod a3_loopback;
mod a4_no_mutation;
mod a5_bounds;
mod a6_semantics;
mod a7_headers_and_logs;
mod a7_source_diagnostics;
mod a8_web_page;
mod contract;
