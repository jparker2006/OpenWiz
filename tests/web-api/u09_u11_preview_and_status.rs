//! U9 and U11 , the server's half.
//!
//! - **U9 effects come before pressing,** in the core's words: the
//!   new-thread preview carries the core's effect lines (or why a create
//!   would stop), and never claims, records or calls `spawn`.
//! - **U11 uncertainty keeps everything, and nothing retries:** an
//!   uncertain create stays uncertain. The status route reads the record
//!   (and, for an uncertain create, the threads seen since: observations
//!   only), and never calls `spawn`; a repeat of the op id replays the
//!   uncertain outcome; there is no retry route (U1).

use serde_json::json;

use crate::new_support::{
    NO_REMOTE, NewFake, PROJECT, TEXT, body, create, preview_json, serve_new, status,
};

#[test]
fn u9_the_preview_carries_the_cores_effects_and_creates_nothing() {
    let fake = NewFake::new("web_u9_effects");
    let s = serve_new(&fake);
    let r = preview_json(&s, PROJECT, &body("u9-op-1", TEXT));
    assert_eq!(r.status, 200, "{}", r.text());
    let preview = r.json();
    assert_eq!(preview["schema"], "openwiz.new-thread-preview.v1");
    assert_eq!(preview["verdict"]["kind"], "ready", "{preview}");
    let effects = preview["effects"]["effects"].as_array().expect("effects");
    let kinds: Vec<&str> = effects
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        [
            "creates_and_starts",
            "starts_from_github",
            "copies_env_files",
            "full_permissions",
            "uses_cloud_agent",
            "mac_stays_awake",
            "cloud_room_names_it",
        ]
    );
    assert!(
        effects[2]["text"].as_str().unwrap().contains(".env"),
        "{preview}"
    );
    // A project a create would refuse: the core's reason, and no effects.
    let r = preview_json(&s, NO_REMOTE, &body("u9-op-2", TEXT));
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(r.json()["verdict"]["kind"], "stopped", "{}", r.text());
    assert!(r.json()["effects"].is_null());
    // Reads only: no spawn, no claim, no record.
    for _ in 0..3 {
        let r = preview_json(&s, PROJECT, &body("u9-op-1", TEXT));
        assert_eq!(r.status, 200);
    }
    assert!(fake.writes().is_empty(), "{:?}", fake.writes());
    assert!(!fake.any_record());
    assert_eq!(fake.spawn_count(), 0);
}

#[test]
fn u11_an_uncertain_create_stays_uncertain_and_nothing_retries() {
    let fake = NewFake::with(
        "web_u11_uncertain",
        vec![json!({"any_spawn": true, "creates": true, "abort": true})],
    );
    let s = serve_new(&fake);
    let r = create(&s, PROJECT, &body("u11-op-1", TEXT));
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(r.json()["status"], "uncertain", "{}", r.text());
    assert_eq!(fake.spawn_count(), 1);
    let record = std::fs::read(fake.record_path("u11-op-1")).expect("the record");
    // Refresh status, as often as the page likes: reads only.
    for _ in 0..3 {
        let r = s.get(&status("u11-op-1"));
        assert_eq!(r.status, 200, "{}", r.text());
        let st = r.json();
        assert_eq!(st["result"]["kind"], "recorded");
        assert_eq!(st["result"]["report"]["status"], "uncertain", "{st}");
        let observations = &st["result"]["observations"];
        assert!(
            observations["text"]
                .as_str()
                .unwrap()
                .starts_with("observations only"),
            "{st}"
        );
    }
    // A repeat of the op id (a reload, a second tab): the recorded outcome.
    let r = create(&s, PROJECT, &body("u11-op-1", TEXT));
    assert_eq!(r.json()["status"], "uncertain");
    assert_eq!(r.json()["result"]["replayed"], true);
    assert_eq!(fake.spawn_count(), 1, "never a second call");
    assert_eq!(
        std::fs::read(fake.record_path("u11-op-1")).expect("the record"),
        record,
        "no status read or repeat changed the record"
    );
}

#[test]
fn u11_the_status_of_a_created_thread_reads_its_start_once() {
    let fake = NewFake::new("web_u11_created");
    let s = serve_new(&fake);
    let r = create(&s, PROJECT, &body("u11-op-2", TEXT));
    assert_eq!(r.json()["status"], "created", "{}", r.text());
    // The create's own answer carries the start, read once.
    let shows = |fake: &NewFake| {
        fake.invocations()
            .iter()
            .filter(|a| a.get(1).is_some_and(|w| w == "show"))
            .count()
    };
    assert_eq!(shows(&fake), 1, "{:?}", fake.invocations());
    assert!(r.json()["start"]["stage"].is_object(), "{}", r.text());
    let r = s.get(&status("u11-op-2"));
    assert_eq!(r.status, 200);
    assert!(
        r.json()["result"]["start"]["stage"].is_object(),
        "{}",
        r.text()
    );
    assert_eq!(shows(&fake), 2);
    assert_eq!(fake.spawn_count(), 1);
}
