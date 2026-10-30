//! OpenWiz core: the inventory model, the honest envelope and the
//! [`source::SessionSource`] interface that adapters implement.
//!
//! The operations in [`ops`] hold the product rules: the version report,
//! same-run grouping and location, visibility, the sort key, paging,
//! page-only enrichment within the budget, the caps and the honest envelope.
//! The core does no I/O of its own: time comes from a [`ops::Clock`], and
//! every read from a [`source::SessionSource`].
//!
//! The core depends on neither an adapter nor a client.

pub mod envelope;
pub mod limits;
pub mod model;
pub mod ops;
pub mod source;
