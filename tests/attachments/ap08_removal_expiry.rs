//! AP8 Removal and expiry. `rm` deletes the bytes and the metadata. A ready
//! attachment no operation has claimed expires 24 hours after it became
//! ready (sheet the check): it is deleted and reported as expired, once. Removal
//! is refused while its operation is dispatching or uncertain, and expiry
//! keeps it then (sheet the check), against a claim source the test sets: the
//! real claims are B2's. Unfinished copies go when the store opens.

use std::ffi::OsString;
use std::fs;

use openwiz_core::attachment::{AttachmentId, Claim, UNSENT_TTL};
use openwiz_core::model::Timestamp;
use openwiz_store::attachments::{NoClaims, PARTIAL_SUFFIX, Removal, Staged};

use crate::bench::{Bench, CONTENT, ClaimsOf, Free, PLENTY, T0};
use crate::support::{PNG, padded};

const TTL_MS: i64 = UNSENT_TTL.as_millis() as i64;

fn add(bench: &Bench, name: &str) -> String {
    let source = bench.source(name, &padded(PNG, 2_000));
    let run = bench.cli(&["add", "--json", source.to_str().unwrap()]);
    assert_eq!(run.code, 0, "{run:?}");
    run.json()["ready"][0]["staged"]["attachment"]["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn at(ms: i64) -> Timestamp {
    Timestamp(T0.0 + ms)
}

#[test]
fn rm_deletes_the_bytes_and_the_metadata() {
    let bench = Bench::new("ap08_rm");
    let id = add(&bench, "syn-ap08-rm.png");
    let other = add(&bench, "syn-ap08-kept.png");
    let dir = bench.attachments().join(&id);
    assert!(dir.join("syn-ap08-rm.png").exists());

    let run = bench.cli(&["rm", &id]);
    assert_eq!(run.code, 0, "{run:?}");
    assert!(run.out.contains(&format!("{id}: removed")), "{}", run.out);
    assert!(!dir.exists(), "the bytes and the metadata are gone");
    assert_eq!(bench.ids(), std::slice::from_ref(&other));

    let ls = bench.cli(&["ls", "--json"]).json();
    let listed: Vec<&str> = ls["attachments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["attachment"]["id"].as_str().unwrap())
        .collect();
    assert_eq!(listed, [other.as_str()]);

    // Again: not found, exit 3, the other untouched.
    let run = bench.cli(&["rm", &id, &other]);
    assert_eq!(run.code, 3, "{run:?}");
    assert!(run.out.contains(&format!("{id}: not found")), "{}", run.out);
    assert!(
        run.out.contains(&format!("{other}: removed")),
        "{}",
        run.out
    );
    assert!(bench.ids().is_empty());
}

#[test]
fn an_unclaimed_attachment_expires_at_its_time_to_live() {
    let bench = Bench::new("ap08_expiry");
    let id = add(&bench, "syn-ap08-expiry.png");

    let ls = bench.cli_at(&["ls", "--json"], at(TTL_MS - 1)).json();
    assert_eq!(ls["attachments"][0]["attachment"]["id"], id.as_str());
    assert_eq!(ls["attachments"][0]["expires_at"], T0.0 + TTL_MS);
    assert_eq!(ls["attachments"][0]["age_ms"], TTL_MS - 1);
    assert!(ls["expired"].as_array().unwrap().is_empty());

    let run = bench.cli_at(&["ls"], at(TTL_MS));
    assert_eq!(run.code, 0, "{run:?}");
    assert!(run.out.contains(&format!("expired {id}")), "{}", run.out);
    assert!(bench.ids().is_empty(), "its bytes are deleted");

    // Reported once: it is gone.
    let ls = bench.cli_at(&["ls", "--json"], at(TTL_MS + 1)).json();
    assert!(ls["attachments"].as_array().unwrap().is_empty());
    assert!(ls["expired"].as_array().unwrap().is_empty());
}

#[test]
fn an_attachment_that_expires_as_rm_runs_is_reported_expired() {
    let bench = Bench::new("ap08_rm_expired");
    let id = add(&bench, "syn-ap08-rm-expired.png");
    let run = bench.cli_at(&["rm", "--json", &id], at(TTL_MS));
    assert_eq!(run.code, 0, "{run:?}");
    let json = run.json();
    assert_eq!(json["ids"][0]["result"], "expired");
    assert_eq!(json["expired"][0]["attachment"]["id"], id.as_str());
    assert!(bench.ids().is_empty());
}

#[test]
fn every_open_expires_and_add_reports_it() {
    let bench = Bench::new("ap08_add_expires");
    let old = add(&bench, "syn-ap08-old.png");
    let source = bench.source("syn-ap08-new.png", &padded(PNG, 2_000));
    let run = bench.cli_at(&["add", "--json", source.to_str().unwrap()], at(TTL_MS));
    assert_eq!(run.code, 0, "{run:?}");
    let json = run.json();
    assert_eq!(json["expired"][0]["attachment"]["id"], old.as_str());
    assert_eq!(json["expired"][0]["readiness"]["state"], "expired");
    let new = json["ready"][0]["staged"]["attachment"]["id"]
        .as_str()
        .unwrap();
    assert_eq!(bench.ids(), [new.to_owned()]);
}

#[test]
fn a_claimed_attachment_is_kept_while_its_operation_may_use_it() {
    let bench = Bench::new("ap08_claimed");
    let claims = ClaimsOf::default();
    let names = [
        "dispatching",
        "uncertain",
        "accepted",
        "failed",
        "unclaimed",
    ];
    let ids: Vec<String> = names
        .iter()
        .map(|n| add(&bench, &format!("syn-ap08-{n}.png")))
        .collect();
    for (id, claim) in ids.iter().zip([
        Claim::Dispatching,
        Claim::Uncertain,
        Claim::Accepted,
        Claim::Failed,
        Claim::Unclaimed,
    ]) {
        claims.set(id, claim);
    }
    let sorted = |mut v: Vec<String>| {
        v.sort();
        v
    };
    let run = |args: &[&str], now: Timestamp| {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        bench.cli_with(&args, now, &Free::new(PLENTY), &claims)
    };

    // Removal is refused while dispatching or uncertain: nothing deleted.
    let store = bench.store();
    for id in &ids[..2] {
        let id = AttachmentId::parse(id).unwrap();
        let removal = store.remove(&id, &claims).unwrap();
        assert!(matches!(removal, Removal::Refused(_)), "{removal:?}");
    }
    assert_eq!(bench.ids().len(), 5, "nothing was deleted");

    // Through the CLI: exit 14. Its open also lets the accepted one go
    // (the check: its operation's outcome is recorded).
    let refused = run(&["rm", &ids[0], &ids[1]], T0);
    assert_eq!(refused.code, 14, "{refused:?}");
    assert!(
        refused.out.contains(&format!("{}: refused", ids[0])),
        "{}",
        refused.out
    );
    assert_eq!(
        bench.ids(),
        sorted(vec![
            ids[0].clone(),
            ids[1].clone(),
            ids[3].clone(),
            ids[4].clone()
        ])
    );

    // Past the time-to-live: the failed and unclaimed ones expire, the
    // dispatching and uncertain ones are kept.
    let ls = run(&["ls", "--json"], at(TTL_MS));
    assert_eq!(ls.code, 0, "{ls:?}");
    let expired: Vec<String> = ls.json()["expired"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["attachment"]["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        sorted(expired),
        sorted(vec![ids[3].clone(), ids[4].clone()])
    );
    assert_eq!(bench.ids(), sorted(vec![ids[0].clone(), ids[1].clone()]));

    // Once its operation is no longer in flight, it can go.
    claims.set(&ids[0], Claim::Accepted);
    claims.set(&ids[1], Claim::Unclaimed);
    let removed = run(&["rm", "--json", &ids[1]], at(TTL_MS - 1));
    assert_eq!(removed.code, 0, "{removed:?}");
    assert_eq!(removed.json()["ids"][0]["result"], "removed");
    assert!(bench.ids().is_empty());
}

#[test]
fn unfinished_copies_go_when_the_store_opens() {
    let bench = Bench::new("ap08_unfinished");
    let id = add(&bench, "syn-ap08-ready.png");
    // A staging that stopped: a directory with a partial and no metadata.
    let stopped = bench.attachments().join("att-synthetic-stopped");
    crate::dirs::private_dir(&stopped).unwrap();
    fs::write(
        stopped.join(format!("syn-ap08-stopped.txt{PARTIAL_SUFFIX}")),
        CONTENT,
    )
    .unwrap();
    let run = bench.cli(&["ls"]);
    assert_eq!(run.code, 0, "{run:?}");
    assert!(!stopped.exists(), "the unfinished copy is deleted");
    assert_eq!(bench.ids(), [id]);
    // Never listed as anything.
    assert!(!run.out.contains("att-synthetic-stopped"), "{}", run.out);

    // A sweep finds nothing more.
    let swept = bench.store().sweep(T0, &NoClaims).unwrap();
    assert_eq!(swept.unfinished, 0);
    assert!(matches!(
        bench.store().stage(
            &bench.source("syn-ap08-after.txt", CONTENT.as_bytes()),
            T0,
            &Free::new(PLENTY)
        ),
        Ok(Staged::Ready(_))
    ));
}
