//! The approved limits, as literals, so a changed constant is caught.
//!
//! `MAX_LIMIT` and `FIELD_MAX_CHARS` are provisional, not approved, so they
//! are not pinned here. P7 still asserts `FIELD_MAX_CHARS` is 200, so that a
//! change to the provisional value is visible in review.

use std::time::Duration;

use openwiz_core::limits::{
    DEFAULT_LIMIT, FINAL_OUTPUT_MAX_BYTES, MAX_CONCURRENT_DETAIL_READS, PURPOSE_MAX_CHARS,
    RUN_BUDGET,
};
use openwiz_core::ops::ListRequest;

#[test]
fn the_approved_limits_are_in_the_core() {
    assert_eq!(PURPOSE_MAX_CHARS, 120);
    assert_eq!(FINAL_OUTPUT_MAX_BYTES, 4096);
    assert_eq!(MAX_CONCURRENT_DETAIL_READS, 4);
    assert_eq!(RUN_BUDGET, Duration::from_secs(30));
    assert_eq!(DEFAULT_LIMIT, 100, "the default page size");
    assert_eq!(
        ListRequest::default().limit,
        100,
        "a request with no --limit"
    );
}
