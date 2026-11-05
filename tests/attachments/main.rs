//! The OpenWiz 0.0.4 attachment proof cases (AP), the implementation. Each case
//! was written before the behaviour, and its first failing run is kept
//! under the release checks (phase 1) and `b1-phase-2/`.
//!
//! **Phase 1** (the model in the core, no I/O): AP1 to AP3, AP10 and AP11's
//! schema half; `rules` holds the readiness, retention and free-disk rules.
//! **Phase 2** (staging in the store, `openwiz attachment add, ls, rm`):
//! AP4 to AP9, AP11's CLI half, AP12, AP2's store half and AP10 again; the
//! streaming SHA-256 staging hashes with (`sha256_stream`).
//!
//! **Fixtures only.** The synthetic files are written at test time into a
//! private directory outside the repository (never committed); nothing
//! calls `room-cli`, and fixture tests never prove CloudRoom's behaviour:
//! its limits and typing rules are source-read,
//! rechecked at the live check (AL2). `bench` runs the store and the CLI on
//! a private state directory, with a faked free-space measure and claim
//! source; `child` is this test binary run again as its own process (AP4
//! stops it at each staging step, AP6 measures its memory). `program` runs
//! the release `openwiz attachment` with the real free-space measure.

mod bench;
mod child;
mod support;

#[allow(dead_code)]
#[path = "../support/schema.rs"]
mod schema;

#[allow(dead_code)]
#[path = "../support/dirs.rs"]
mod dirs;

// `pattern`, for the attachment schema only.
mod pattern;

mod ap01_kind;
mod ap02_limits;
mod ap03_safe_names;
mod ap04_ready;
mod ap05_originals;
mod ap06_streaming;
mod ap07_low_disk;
mod ap08_removal_expiry;
mod ap09_owner_only;
mod ap10_no_dispatch;
mod ap11_cli;
mod ap11_schema;
mod ap12_logs;
mod parity;
mod program;
mod rules;
mod sha256_stream;

// B2 (the implementation): the rules and static checks of attachments in send
// and new_thread; their end-to-end proofs are in the messaging and
// new-thread targets, which share `staging`.
mod ad_static;
