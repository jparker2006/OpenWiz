//! OpenWiz core: the inventory model, the honest envelope and the
//! [`source::SessionSource`] interface that adapters implement.
//!
//! The operations in [`ops`] hold the product rules: the version pin,
//! same-run grouping and location, visibility, the sort key, paging,
//! page-only enrichment within the budget, the caps and the honest envelope.
//! The core does no I/O of its own: time comes from a [`ops::Clock`], and
//! every read from a [`source::SessionSource`].
//!
//! [`messaging`] holds OpenWiz's one write, `send`: its operation, outcomes,
//! effects, validation, op-id rules, reconciliation rule and the interface
//! of the send record. It does no I/O either.
//!
//! The core depends on neither an adapter nor a client.

pub mod attachment;
pub mod envelope;
pub mod limits;
pub mod messaging;
pub mod model;
pub mod new_thread;
pub mod ops;
pub mod prompt;
pub mod source;
