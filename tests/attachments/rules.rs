//! The readiness, retention and free-disk rules that phase
//! 2's store follows. Pure functions here; AP4, AP7 and AP8 prove the store
//! applies them.

use std::time::Duration;

use openwiz_core::attachment::{
    Claim, DeleteReason, FREE_DISK_FLOOR_BYTES, FailReason, Readiness, RemoveRefused, Retention,
    Stored, UNSENT_TTL, can_remove, retention, staging_room,
};

use crate::support::MIB;

const HOUR: Duration = Duration::from_secs(3600);

fn every_state() -> Vec<Readiness> {
    vec![
        Readiness::Selected,
        Readiness::Uploading,
        Readiness::Ready,
        Readiness::Failed {
            reason: FailReason::ConnectionLost,
        },
        Readiness::Removed,
        Readiness::Expired,
    ]
}

#[test]
fn only_ready_claims_a_verified_copy() {
    for state in every_state() {
        assert_eq!(state.is_ready(), state == Readiness::Ready, "{state:?}");
    }
}

#[test]
fn the_readiness_transitions() {
    use Readiness::*;
    let failed = Failed {
        reason: FailReason::SizeMismatch,
    };
    let allowed = [
        (Selected, Uploading),
        (Selected, failed),
        (Selected, Removed),
        (Uploading, Ready),
        (Uploading, failed),
        (Uploading, Removed),
        (Ready, failed),
        (Ready, Removed),
        (Ready, Expired),
        (failed, Uploading),
        (failed, Removed),
    ];
    let mut states = every_state();
    states.push(failed);
    for from in &states {
        for to in &states {
            let expected = allowed.contains(&(*from, *to))
                || (matches!(from, Selected | Uploading | Ready) && matches!(to, Failed { .. }))
                || (matches!(from, Failed { .. }) && matches!(to, Uploading | Removed));
            assert_eq!(from.can_become(*to), expected, "{from:?} -> {to:?}");
        }
    }
    // Nothing becomes ready without an upload, and nothing leaves removed
    // or expired.
    assert!(!Selected.can_become(Ready));
    assert!(!failed.can_become(Ready));
    for end in [Removed, Expired] {
        for to in &states {
            assert!(!end.can_become(*to), "{end:?} -> {to:?}");
        }
    }
}

#[test]
fn an_unsent_ready_attachment_expires_after_24_hours() {
    assert_eq!(UNSENT_TTL, 24 * HOUR, "B-2");
    let stored = |age| Stored::Ready {
        claim: Claim::Unclaimed,
        age,
    };
    assert_eq!(retention(stored(Duration::ZERO)), Retention::Keep);
    assert_eq!(
        retention(stored(UNSENT_TTL - Duration::from_millis(1))),
        Retention::Keep
    );
    assert_eq!(
        retention(stored(UNSENT_TTL)),
        Retention::Delete(DeleteReason::Expired)
    );
    assert_eq!(
        retention(stored(30 * 24 * HOUR)),
        Retention::Delete(DeleteReason::Expired)
    );
}

#[test]
fn a_claimed_attachment_follows_its_operation() {
    // deleted once an accepted outcome is recorded; kept on a failure
    // until the TTL; kept while dispatching or uncertain, past the TTL too.
    for age in [Duration::ZERO, UNSENT_TTL, 30 * 24 * HOUR] {
        let at = |claim| retention(Stored::Ready { claim, age });
        assert_eq!(
            at(Claim::Accepted),
            Retention::Delete(DeleteReason::Accepted)
        );
        assert_eq!(at(Claim::Dispatching), Retention::Keep);
        assert_eq!(at(Claim::Uncertain), Retention::Keep);
        let failed = if age >= UNSENT_TTL {
            Retention::Delete(DeleteReason::Expired)
        } else {
            Retention::Keep
        };
        assert_eq!(at(Claim::Failed), failed, "{age:?}");
    }
}

#[test]
fn a_partial_copy_is_always_deleted() {
    assert_eq!(
        retention(Stored::Partial),
        Retention::Delete(DeleteReason::Partial)
    );
}

#[test]
fn removal_is_refused_while_an_operation_may_still_use_it() {
    assert_eq!(can_remove(Claim::Unclaimed), Ok(()));
    assert_eq!(can_remove(Claim::Failed), Ok(()));
    assert_eq!(can_remove(Claim::Accepted), Ok(()));
    assert_eq!(
        can_remove(Claim::Dispatching),
        Err(RemoveRefused(Claim::Dispatching))
    );
    assert_eq!(
        can_remove(Claim::Uncertain),
        Err(RemoveRefused(Claim::Uncertain))
    );
}

#[test]
fn staging_keeps_1_gib_free() {
    assert_eq!(FREE_DISK_FLOOR_BYTES, 1024 * MIB, "B-3");
    let floor = FREE_DISK_FLOOR_BYTES;
    assert_eq!(staging_room(floor + 25 * MIB, 25 * MIB), Ok(()));
    assert_eq!(
        staging_room(floor + 25 * MIB - 1, 25 * MIB),
        Err(FailReason::LowDisk)
    );
    assert_eq!(staging_room(floor, 0), Ok(()));
    assert_eq!(staging_room(floor - 1, 0), Err(FailReason::LowDisk));
    assert_eq!(staging_room(0, 1), Err(FailReason::LowDisk));
    assert_eq!(staging_room(u64::MAX, u64::MAX), Err(FailReason::LowDisk));
    assert!(FailReason::LowDisk.message().contains("1 GiB"));
}
