use std::fs;
use std::time::Duration;

use openwiz_core::attachment::{Claim, PromptAttachment};
use openwiz_core::messaging::record::{SendRecord, SendStore, StoreError};
use openwiz_core::messaging::send::{
    self, Delivery, STEER_WITH_ATTACHMENTS, SendError, SendReport, SendRequest, SendResult, Status,
};
use openwiz_core::messaging::{Binding, BindingField, Mode, OpId};
use openwiz_core::model::Timestamp;
use openwiz_core::prompt::{NOT_REPORTED, attachments_digest};
use openwiz_store::FileStore;
use openwiz_store::attachments::{Claims, ReadyAttachment, Removal};
use serde_json::{Value, json};

use crate::staging::{self, copy_path, id_of, kept, png, stage, text, uploads};
use crate::support::*;

const CLOUD: &str = "thr_syn_0001";
const LOCAL: &str = "thr_syn_0002";
const HOUR_MS: i64 = 60 * 60 * 1000;

/// A tell to `target` in queue mode with any text and any attachments,
/// answered with the `send` scenario's `file`, and `rest`.
fn tell(target: &str, file: &str, rest: Value) -> Value {
    let mut response = tell_response(target, "queue", json!({ "any_attachments": true }));
    response["stdout_file"] = json!(send_fixtures().join(file).to_str().unwrap());
    for (k, v) in rest.as_object().unwrap() {
        response[k] = v.clone();
    }
    response
}

fn with_ids(mut r: SendRequest, ready: &[&ReadyAttachment]) -> SendRequest {
    r.prompt.attachments = ready.iter().map(|a| a.attachment.id.clone()).collect();
    r
}

/// `send_with`, the attachment store under the fake's state directory.
fn send_with(fake: &SendFake, request: &SendRequest) -> Result<SendReport, SendError> {
    send::send_with(
        &fake.source(),
        &fake.store(),
        &staging::store(&fake.state),
        &clock(),
        request,
    )
}

fn report_with(fake: &SendFake, request: &SendRequest) -> SendReport {
    send_with(fake, request).unwrap_or_else(|e| panic!("send failed: {e:?}"))
}

fn refusal_of(fake: &SendFake, request: &SendRequest) -> String {
    match send_with(fake, request) {
        Err(SendError::Invalid(why)) => why.0,
        other => panic!("not refused: {other:?}"),
    }
}

fn delivery_of(report: &SendReport) -> &Delivery {
    let SendResult::Outcome { delivery, .. } = &report.result else {
        panic!("no outcome: {report:?}")
    };
    delivery
}

fn bound(ready: &[&ReadyAttachment]) -> Vec<PromptAttachment> {
    ready.iter().map(|a| (&a.attachment).into()).collect()
}

/// The claim recorded for `ready`, as the store judges it now.
fn claim_state(fake: &SendFake, ready: &ReadyAttachment, now: Timestamp) -> Claim {
    staging::store(&fake.state)
        .recorded_claims(now)
        .claim(&ready.attachment.id)
        .unwrap()
}

// AD1: the exact invocation.

#[test]
fn ad01_one_tell_carries_every_staged_copy_files_first_then_images() {
    let fake = SendFake::with("ad01_tell", vec![tell(CLOUD, "tell-sent.json", json!({}))]);
    let a = stage(&fake.state, "syn-ad01-a.png", &png(2048), T0);
    let notes = stage(
        &fake.state,
        "syn-ad01-notes.txt",
        &text("Synthetic notes.\n"),
        T0,
    );
    let b = stage(&fake.state, "syn-ad01-b.png", &png(4096), T0);
    let r = with_ids(
        request(CLOUD, Mode::Queue, "Synthetic with three", "op-ad01"),
        &[&a, &notes, &b],
    );
    let report = report_with(&fake, &r);
    assert_eq!(report.status, Status::Accepted, "{report:?}");

    // One call: the tail from attachment_args before `--`, files first.
    let tells = fake.tells();
    assert_eq!(tells.len(), 1);
    let path = |r: &ReadyAttachment| copy_path(&fake.state, r).to_str().unwrap().to_owned();
    let mut expected: Vec<String> = ["thread", "tell", CLOUD, "--mode", "queue", "--json"]
        .iter()
        .map(|w| (*w).to_owned())
        .collect();
    for (flag, r) in [("--file", &notes), ("--image", &a), ("--image", &b)] {
        expected.push(flag.to_owned());
        expected.push(path(r));
    }
    expected.extend(["--".to_owned(), "Synthetic with three".to_owned()]);
    assert_eq!(tells[0], expected);

    // Each path is absolute and its bytes, read by the fake at call time,
    // are the staged copy's.
    let log = uploads(&fake.dir);
    assert_eq!(log.len(), 1);
    let items = log[0]["items"].as_array().unwrap();
    for (item, r) in items.iter().zip([&notes, &a, &b]) {
        assert!(item["path"].as_str().unwrap().starts_with('/'));
        assert_eq!(item["sha256"], r.attachment.sha256.as_str());
        assert_eq!(item["size"], r.attachment.size_bytes);
    }

    // The report: the owner's order, the digest, and no stronger claim.
    assert_eq!(report.attachments, bound(&[&a, &notes, &b]));
    assert_eq!(
        report.attachments_digest.as_deref(),
        Some(attachments_digest(&bound(&[&a, &notes, &b])).as_str())
    );
    let words = delivery_of(&report).text();
    assert!(
        words.contains("with 3 attachments handed to CloudRoom") && words.contains(NOT_REPORTED),
        "{words}"
    );
}

#[test]
fn ad01_a_path_in_the_text_is_sent_as_text_never_attached() {
    let fake = SendFake::with(
        "ad01_path_text",
        vec![tell(CLOUD, "tell-sent.json", json!({}))],
    );
    let notes = stage(&fake.state, "syn-ad01-path.txt", &text("Synthetic.\n"), T0);
    let path = copy_path(&fake.state, &notes).to_str().unwrap().to_owned();
    for (i, words) in [
        path.clone(),
        format!("file://{path}"),
        format!("see {path}"),
    ]
    .iter()
    .enumerate()
    {
        let report = report_with(
            &fake,
            &request(CLOUD, Mode::Queue, words, &format!("op-ad01-text-{i}")),
        );
        assert_eq!(report.status, Status::Accepted);
        assert!(report.attachments.is_empty());
    }
    for argv in fake.tells() {
        assert!(
            !argv.iter().any(|w| w == "--file" || w == "--image"),
            "{argv:?}"
        );
    }
    assert!(uploads(&fake.dir).is_empty(), "nothing was uploaded");
    assert!(kept(&fake.state, &notes), "the staged copy is untouched");
}

// AD2 and AD3: all ready or nothing; no partial send.

/// The ways an attachment can be not ready, each made in its own way.
fn unready(fake: &SendFake, tag: &str) -> Vec<(String, &'static str)> {
    let mut out = Vec::new();
    // Never staged.
    out.push((format!("att-{tag}-missing"), "is not found"));
    // Removed by the owner.
    let removed = stage(
        &fake.state,
        &format!("syn-{tag}-removed.txt"),
        b"removed\n",
        T0,
    );
    let store = staging::store(&fake.state);
    let claims = store.recorded_claims(T0);
    assert!(matches!(
        store.remove(&removed.attachment.id, &claims).unwrap(),
        Removal::Removed(_)
    ));
    out.push((id_of(&removed), "is not found"));
    // Ready 25 hours ago and never claimed: expired, not yet swept.
    let expired = stage(
        &fake.state,
        &format!("syn-{tag}-expired.txt"),
        b"expired\n",
        Timestamp(T0.0 - 25 * HOUR_MS),
    );
    out.push((id_of(&expired), "is expired"));
    // Its copy rewritten: other bytes than its record.
    let tampered = stage(
        &fake.state,
        &format!("syn-{tag}-tampered.txt"),
        b"before\n",
        T0,
    );
    staging::tamper(&fake.state, &tampered, b"after!!\n");
    out.push((id_of(&tampered), "is not ready"));
    // Its metadata broken: a failed attachment.
    let broken = stage(
        &fake.state,
        &format!("syn-{tag}-broken.txt"),
        b"broken\n",
        T0,
    );
    let meta = fake
        .state
        .join("attachments")
        .join(id_of(&broken))
        .join(".meta.json");
    fs::write(&meta, b"{\"not\": \"a record\"}").unwrap();
    out.push((id_of(&broken), "is not ready"));
    out
}

#[test]
fn ad02_each_unready_attachment_gives_zero_calls_and_is_named() {
    let fake = SendFake::with("ad02_each", vec![tell(CLOUD, "tell-sent.json", json!({}))]);
    let good = stage(&fake.state, "syn-ad02-good.txt", &text("good\n"), T0);
    for (i, (id, words)) in unready(&fake, "ad02").into_iter().enumerate() {
        let mut r = request(CLOUD, Mode::Queue, "Synthetic", &format!("op-ad02-{i}"));
        r.prompt.attachments = vec![good.attachment.id.clone(), staging::attachment_id(&id)];
        let why = refusal_of(&fake, &r);
        assert!(
            why.contains(&format!("attachment {id} {words}")),
            "{id}: {why}"
        );
        assert!(why.contains("nothing was sent"), "{why}");
        assert!(
            !why.contains(&id_of(&good)),
            "the good one is not named: {why}"
        );
        // Nothing was claimed, read or sent, and the good one is as it was.
        assert!(
            fake.invocations().is_empty(),
            "{id}: {:?}",
            fake.invocations()
        );
        assert!(!fake.record_path(&format!("op-ad02-{i}")).exists());
        assert!(kept(&fake.state, &good));
        assert_eq!(staging::claim_of(&fake.state, &good), None);
    }
}

#[test]
fn ad03_one_refusal_names_every_problem_and_no_subset_is_ever_sent() {
    let fake = SendFake::with("ad03_all", vec![tell(CLOUD, "tell-sent.json", json!({}))]);
    let first = stage(&fake.state, "syn-ad03-first.txt", &text("first\n"), T0);
    let last = stage(&fake.state, "syn-ad03-last.png", &png(1024), T0);
    let problems = unready(&fake, "ad03");
    let mut r = request(CLOUD, Mode::Queue, "Synthetic", "op-ad03");
    r.prompt.attachments = std::iter::once(first.attachment.id.clone())
        .chain(problems.iter().map(|(id, _)| staging::attachment_id(id)))
        .chain(std::iter::once(last.attachment.id.clone()))
        .collect();
    let why = refusal_of(&fake, &r);
    for (id, words) in &problems {
        assert!(why.contains(&format!("attachment {id} {words}")), "{why}");
    }
    assert_eq!(fake.tell_count(), 0, "no subset was sent");
    assert!(uploads(&fake.dir).is_empty());
    // The owner's explicit new choice, with only the ready ones, under a
    // new op id: one tell carrying exactly those.
    let r = with_ids(
        request(CLOUD, Mode::Queue, "Synthetic", "op-ad03-ready"),
        &[&first, &last],
    );
    assert_eq!(report_with(&fake, &r).status, Status::Accepted);
    let tells = fake.tells();
    assert_eq!(tells.len(), 1);
    assert_eq!(
        tells[0]
            .iter()
            .filter(|w| *w == "--file" || *w == "--image")
            .count(),
        2
    );
}

// AD4: the binding.

#[test]
fn ad04_same_op_id_and_payload_is_one_call_any_attachment_difference_is_refused() {
    let fake = SendFake::with(
        "ad04_binding",
        vec![tell(CLOUD, "tell-sent.json", json!({}))],
    );
    let a = stage(&fake.state, "syn-ad04-a.txt", &text("a\n"), T0);
    let b = stage(&fake.state, "syn-ad04-b.png", &png(1024), T0);
    let c = stage(&fake.state, "syn-ad04-c.txt", &text("c\n"), T0);
    // Same name and kind as `a`, other bytes: another staged copy.
    let a2 = stage(&fake.state, "syn-ad04-a.txt", &text("A\n"), T0);
    let base = with_ids(
        request(CLOUD, Mode::Queue, "Synthetic", "op-ad04"),
        &[&a, &b],
    );
    assert_eq!(report_with(&fake, &base).status, Status::Accepted);
    assert_eq!(fake.tell_count(), 1);

    // The same payload again: its recorded outcome, no call (its copies
    // are gone after the accepted outcome, the check; the record binds them).
    let again = report_with(&fake, &base);
    assert!(matches!(
        again.result,
        SendResult::Outcome { replayed: true, .. }
    ));
    assert_eq!(again.attachments, bound(&[&a, &b]));

    for (name, ids) in [
        ("added", vec![&a, &b, &c]),
        ("removed", vec![&a]),
        ("reordered", vec![&b, &a]),
        ("changed bytes", vec![&a2, &b]),
        ("none", vec![]),
    ] {
        let r = with_ids(base.clone(), &ids);
        let report = report_with(&fake, &r);
        assert_eq!(report.status, Status::ConflictingReuse, "{name}");
        let SendResult::ConflictingReuse { differs } = &report.result else {
            panic!("{name}: {report:?}")
        };
        assert_eq!(differs, &vec![BindingField::Attachments], "{name}");
    }
    assert_eq!(fake.tell_count(), 1, "no second call");
    // An unused copy is untouched by a refusal.
    assert!(kept(&fake.state, &c) && kept(&fake.state, &a2));
}

#[test]
fn ad04_a_0_0_3_record_without_attachments_still_matches() {
    let fake = SendFake::with(
        "ad04_old_record",
        vec![tell(CLOUD, "tell-sent.json", json!({}))],
    );
    let r = request(CLOUD, Mode::Queue, "Synthetic old", "op-ad04-old");
    // A finished 0.0.3 record, written as 0.0.3 wrote it: no
    // attachments_digest, no attachments.
    let record = SendRecord::claimed(
        r.op_id.clone(),
        Binding::new(r.target.clone(), Mode::Queue, &r.prompt.text),
        &r.prompt.text,
        T0,
    )
    .finished(Delivery::accepted(Mode::Queue), T0);
    let mut json = serde_json::to_value(&record).unwrap();
    assert!(json["binding"].get("attachments_digest").is_none());
    assert!(json.get("attachments").is_none());
    json.as_object_mut().unwrap().remove("source");
    // The state directory and sends/, owner only, whatever the umask .
    crate::dirs::private_dir_all(fake.state.join("sends")).unwrap();
    let store = fake.store();
    fs::write(
        store.record_path(&r.op_id),
        serde_json::to_vec(&json).unwrap(),
    )
    .unwrap();
    let report = report_with(&fake, &r);
    assert!(
        matches!(report.result, SendResult::Outcome { replayed: true, .. }),
        "{report:?}"
    );
    // With an attachment, the same op id is conflicting reuse.
    let a = stage(&fake.state, "syn-ad04-old.txt", &text("a\n"), T0);
    let report = report_with(&fake, &with_ids(r, &[&a]));
    assert_eq!(report.status, Status::ConflictingReuse);
    assert_eq!(fake.tell_count(), 0);
}

// AD5: Steer with attachments.

#[test]
fn ad05_steer_with_attachments_is_refused_with_zero_calls_local_and_cloud() {
    let fake = SendFake::new("ad05_steer", "send");
    let a = stage(&fake.state, "syn-ad05.txt", &text("a\n"), T0);
    for (i, target) in [CLOUD, LOCAL].into_iter().enumerate() {
        let r = with_ids(
            request(
                target,
                Mode::Steer,
                "Synthetic steer",
                &format!("op-ad05-{i}"),
            ),
            &[&a],
        );
        assert_eq!(refusal_of(&fake, &r), STEER_WITH_ATTACHMENTS);
        let id = id_of(&a);
        let (code, out, err) = fake.cli(
            &[
                target,
                "--steer",
                "--attachment",
                &id,
                "--",
                "Synthetic steer",
            ],
            "",
        );
        assert_eq!(code, 2, "{out}{err}");
        assert!(err.contains("Steer can't carry attachments"), "{err}");
    }
    assert!(fake.invocations().is_empty());
    assert_eq!(staging::claim_of(&fake.state, &a), None);
}

// AD7: the outcome table.

#[test]
fn ad07_every_row_of_the_outcome_table() {
    let fake = SendFake::with(
        "ad07_outcomes",
        vec![
            tell(CLOUD, "tell-sent.json", json!({})),
            tell(LOCAL, "tell-queue-observed-0002.json", json!({})),
            tell(
                "thr_syn_0005",
                "tell-sent.json",
                json!({ "exit": 1, "stderr": "Error: HTTP 400: Invalid input" }),
            ),
            tell("thr_syn_0004", "tell-sent.json", json!({ "hang": true })),
            tell(
                "thr_syn_0003",
                "tell-sent.json",
                json!({ "upload_fails_after": 1 }),
            ),
            tell(
                "thr_syn_0006",
                "tell-sent.json",
                json!({ "exit": 1, "stderr": "Error: HTTP 413: Attachment too large" }),
            ),
        ],
    );
    let case = |target: &str, op: &str| {
        let a = stage(&fake.state, &format!("syn-{op}-a.txt"), &text("a\n"), T0);
        let b = stage(&fake.state, &format!("syn-{op}-b.png"), &png(1024), T0);
        let mut r = with_ids(request(target, Mode::Queue, "Synthetic", op), &[&a, &b]);
        r.budget = Duration::from_secs(2);
        (report_with(&fake, &r), a, b)
    };
    // Sent: accepted, the attachments handed to CloudRoom, nothing more.
    let (sent, ..) = case(CLOUD, "op-ad07-sent");
    assert_eq!(sent.status, Status::Accepted);
    assert!(
        delivery_of(&sent)
            .text()
            .contains("with 2 attachments handed to CloudRoom")
    );
    // Observed busy queue acceptance records the outcome before cleanup.
    let (queued, a, b) = case(LOCAL, "op-ad07-queued");
    assert_eq!(queued.status, Status::Queued);
    assert!(
        delivery_of(&queued)
            .text()
            .starts_with("queued: thread busy, with 2 attachments handed to CloudRoom")
    );
    assert!(delivery_of(&queued).text().contains(NOT_REPORTED));
    assert!(!kept(&fake.state, &a));
    assert!(!kept(&fake.state, &b));
    // A documented rejection before acceptance: failed.
    let (rejected, ..) = case("thr_syn_0005", "op-ad07-rejected");
    assert_eq!(rejected.status, Status::Failed);
    // Uploaded, then no answer in time: uncertain.
    let (timed_out, ..) = case("thr_syn_0004", "op-ad07-timeout");
    assert_eq!(timed_out.status, Status::Uncertain);
    assert!(
        matches!(delivery_of(&timed_out), Delivery::Uncertain { reason, .. }
        if *reason == openwiz_core::messaging::send::UncertainReason::TimedOut)
    );
    // The upload fails part way, with an undocumented error: uncertain.
    let (upload, ..) = case("thr_syn_0003", "op-ad07-upload");
    assert_eq!(upload.status, Status::Uncertain);
    // An attachment refusal no pinned fixture documents: uncertain.
    let (undocumented, ..) = case("thr_syn_0006", "op-ad07-undocumented");
    assert_eq!(undocumented.status, Status::Uncertain);
    for report in [&rejected, &timed_out, &upload, &undocumented] {
        assert!(!delivery_of(report).text().contains("handed to CloudRoom"));
    }
    // Every call carried both attachments, uploaded first.
    assert_eq!(fake.tell_count(), 6);
}

// AD9: retention.

/// A send store whose writes of a finished record fail: the outcome is
/// never recorded.
struct OutcomeNotRecorded(FileStore);

impl SendStore for OutcomeNotRecorded {
    fn load(&self, op_id: &OpId) -> Result<Option<SendRecord>, StoreError> {
        self.0.load(op_id)
    }
    fn claim(
        &self,
        record: &SendRecord,
        now: Timestamp,
    ) -> Result<openwiz_core::messaging::record::Claim, StoreError> {
        self.0.claim(record, now)
    }
    fn update(&self, record: &SendRecord) -> Result<(), StoreError> {
        if record.state == openwiz_core::messaging::record::RecordState::Finished {
            return Err(StoreError(
                "synthetic: the disk refused the write".to_owned(),
            ));
        }
        self.0.update(record)
    }
    fn release(&self, op_id: &OpId) {
        self.0.release(op_id);
    }
    fn uncertain_matching(
        &self,
        binding: &Binding,
        except: &OpId,
        now: Timestamp,
    ) -> Result<Vec<SendRecord>, StoreError> {
        self.0.uncertain_matching(binding, except, now)
    }
    fn in_flight_matching(
        &self,
        binding: &Binding,
        except: &OpId,
        now: Timestamp,
    ) -> Result<Vec<SendRecord>, StoreError> {
        self.0.in_flight_matching(binding, except, now)
    }
    fn inspect(&self, op_id: &OpId, now: Timestamp) -> Result<Option<SendRecord>, StoreError> {
        self.0.inspect(op_id, now)
    }
    fn withdraw(&self, op_id: &OpId) -> Result<(), StoreError> {
        self.0.withdraw(op_id)
    }
}

#[test]
fn ad09_retention_follows_the_table() {
    let fake = SendFake::with(
        "ad09_retention",
        vec![
            tell(CLOUD, "tell-sent.json", json!({})),
            tell(
                "thr_syn_0005",
                "tell-sent.json",
                json!({ "exit": 1, "stderr": "Error: HTTP 400: Invalid input" }),
            ),
            tell(
                "thr_syn_0006",
                "tell-sent.json",
                json!({ "exit": 1, "stderr": "Error: HTTP 500: Synthetic" }),
            ),
        ],
    );
    let later = Timestamp(T0.0 + 25 * HOUR_MS);

    // Accepted: deleted once the outcome is recorded, and only then; the
    // copy was there at the call, and the record keeps its hash.
    let sent = stage(&fake.state, "syn-ad09-sent.txt", &text("sent\n"), T0);
    let report = report_with(
        &fake,
        &with_ids(request(CLOUD, Mode::Queue, "S", "op-ad09-sent"), &[&sent]),
    );
    assert_eq!(report.status, Status::Accepted);
    assert_eq!(
        uploads(&fake.dir)[0]["items"][0]["sha256"],
        sent.attachment.sha256.as_str()
    );
    assert!(
        !kept(&fake.state, &sent),
        "an accepted send's copy is deleted"
    );
    assert_eq!(
        fake.record("op-ad09-sent")["attachments"][0]["sha256"],
        sent.attachment.sha256.as_str()
    );

    // Failed before acceptance: kept until the time-to-live.
    let failed = stage(&fake.state, "syn-ad09-failed.txt", &text("failed\n"), T0);
    let report = report_with(
        &fake,
        &with_ids(
            request("thr_syn_0005", Mode::Queue, "S", "op-ad09-failed"),
            &[&failed],
        ),
    );
    assert_eq!(report.status, Status::Failed);
    assert!(kept(&fake.state, &failed));
    assert_eq!(claim_state(&fake, &failed, T0), Claim::Failed);
    assert_eq!(
        staging::claim_of(&fake.state, &failed),
        Some(json!({"kind": "messaging", "op_id": "op-ad09-failed"}))
    );

    // Uncertain: kept until resolved, past the time-to-live too, and the
    // owner can't remove it.
    let uncertain = stage(
        &fake.state,
        "syn-ad09-uncertain.txt",
        &text("uncertain\n"),
        T0,
    );
    let report = report_with(
        &fake,
        &with_ids(
            request("thr_syn_0006", Mode::Queue, "S", "op-ad09-uncertain"),
            &[&uncertain],
        ),
    );
    assert_eq!(report.status, Status::Uncertain);
    assert_eq!(claim_state(&fake, &uncertain, later), Claim::Uncertain);
    let store = staging::store(&fake.state);
    assert_eq!(
        store
            .remove(&uncertain.attachment.id, &store.recorded_claims(T0))
            .unwrap(),
        Removal::Refused(Claim::Uncertain)
    );

    // The sweep at the time-to-live: the failed one goes, the uncertain
    // one stays.
    let swept = store.sweep(later, &store.recorded_claims(later)).unwrap();
    assert_eq!(swept.expired.len(), 1);
    assert!(!kept(&fake.state, &failed));
    assert!(kept(&fake.state, &uncertain));

    // The outcome can't be recorded: nothing is deleted.
    let unrecorded = stage(
        &fake.state,
        "syn-ad09-unrecorded.txt",
        &text("unrecorded\n"),
        T0,
    );
    let r = with_ids(
        request(CLOUD, Mode::Queue, "S unrecorded", "op-ad09-unrecorded"),
        &[&unrecorded],
    );
    let report = send::send_with(
        &fake.source(),
        &OutcomeNotRecorded(fake.store()),
        &staging::store(&fake.state),
        &clock(),
        &r,
    )
    .unwrap();
    assert!(
        report
            .gaps
            .iter()
            .any(|g| g.detail.contains("could not be recorded"))
    );
    assert!(
        kept(&fake.state, &unrecorded),
        "deleted before its outcome was recorded"
    );
}

// AD10: crash recovery.

#[test]
fn ad10_a_crash_at_each_step_keeps_the_attachments_and_dispatches_nothing() {
    for (crash, response, expected) in [
        (
            "after-claim",
            json!({ "argv": ["--version"], "kill_parent": true }),
            "failed",
        ),
        (
            "during-call",
            tell(CLOUD, "tell-sent.json", json!({ "kill_parent": true })),
            "uncertain",
        ),
        (
            "after-call",
            tell(
                CLOUD,
                "tell-sent.json",
                json!({ "kill_parent_after_answer": true }),
            ),
            "uncertain",
        ),
    ] {
        let fake = SendFake::with(&format!("ad10_{crash}"), vec![response]);
        // A release process: its clock is the real one.
        let now = staging::now();
        let a = stage(
            &fake.state,
            &format!("syn-ad10-{crash}.txt"),
            &text("a\n"),
            now,
        );
        let op = format!("op-ad10-{crash}");
        let id = id_of(&a);
        let args = [
            "send",
            CLOUD,
            "--op-id",
            &op,
            "--attachment",
            &id,
            "--",
            "Synthetic",
        ];
        let first = fake.run_process(&args, "");
        assert_eq!(first.status.code(), None, "{crash}: killed");
        let tells = fake.tell_count();
        // The same op id again, with the fake answering normally now: the
        // recovered outcome, with no new call.
        fake.override_responses(vec![tell(CLOUD, "tell-sent.json", json!({}))]);
        let mut json_args = args.to_vec();
        json_args.insert(json_args.len() - 2, "--json");
        let (code, report) = json_of(&fake.run_process(&json_args, ""));
        assert_eq!(report["status"], expected, "{crash}: {report}");
        assert_eq!(report["result"]["recovered"], true, "{crash}");
        assert_eq!(
            fake.tell_count(),
            tells,
            "{crash}: nothing was dispatched again"
        );
        assert_ne!(code, Some(0), "{crash}");
        assert!(kept(&fake.state, &a), "{crash}: the attachment is kept");
        let claim = claim_state(&fake, &a, now);
        let want = if expected == "failed" {
            Claim::Failed
        } else {
            Claim::Uncertain
        };
        assert_eq!(claim, want, "{crash}");
    }
}

// AD13: the CLI.

#[test]
fn ad13_attach_that_fails_on_any_path_sends_nothing_and_prints_every_result() {
    let fake = SendFake::new("ad13_attach_fails", "send");
    let dir = fake.dir.join("sources");
    crate::dirs::private_dir_all(&dir).unwrap();
    let good = dir.join("syn-ad13-good.txt");
    fs::write(&good, "good\n").unwrap();
    let heic = dir.join("syn-ad13-photo.heic");
    fs::write(&heic, b"\0\0\0\x18ftypheic\0\0\0\0mif1heic").unwrap();
    for json in [false, true] {
        let mut args = vec![
            CLOUD,
            "--attach",
            good.to_str().unwrap(),
            "--attach",
            heic.to_str().unwrap(),
        ];
        if json {
            args.push("--json");
        }
        args.extend(["--", "Synthetic"]);
        let (code, out, err) = fake.cli(&args, "");
        assert_eq!(code, 5, "{out}{err}");
        assert!(
            out.contains("syn-ad13-good.txt") && out.contains("syn-ad13-photo.heic"),
            "{out}"
        );
        if json {
            let value: Value = serde_json::from_str(&out).expect("JSON");
            assert_eq!(value["schema"], "openwiz.attachment-add.v1");
        } else {
            assert!(out.contains("nothing was sent"), "{out}");
        }
    }
    assert!(fake.invocations().is_empty(), "nothing was read or sent");
}

#[test]
fn ad13_attach_and_attachment_send_one_tell_and_the_json_matches_the_text() {
    let fake = SendFake::with("ad13_cli", vec![tell(CLOUD, "tell-sent.json", json!({}))]);
    let staged = stage(&fake.state, "syn-ad13-staged.png", &png(2048), T0);
    let dir = fake.dir.join("sources");
    crate::dirs::private_dir_all(&dir).unwrap();
    let notes = dir.join("syn-ad13-notes.txt");
    fs::write(&notes, "Synthetic notes.\n").unwrap();
    let id = id_of(&staged);
    let base = [
        CLOUD,
        "--op-id",
        "op-ad13",
        "--attachment",
        id.as_str(),
        "--attach",
        notes.to_str().unwrap(),
    ];
    let mut args = base.to_vec();
    args.extend(["--", "Synthetic"]);
    let (code, out, err) = fake.cli(&args, "");
    assert_eq!(code, 0, "{out}{err}");
    let tells = fake.tells();
    assert_eq!(tells.len(), 1);
    assert_eq!(tells[0][6], "--file", "files first: {:?}", tells[0]);
    assert_eq!(tells[0][8], "--image");
    // The same op id again, as JSON: the recorded report, which the text
    // matched line for line.
    let mut json_args = base.to_vec();
    json_args.extend(["--json", "--", "Synthetic"]);
    let (code, report) = fake.cli_json(&json_args, "");
    assert_eq!(code, 0);
    assert_eq!(validate_report(&report), Vec::<String>::new(), "{report}");
    let attachments = report["attachments"].as_array().unwrap();
    assert_eq!(attachments.len(), 2);
    assert_eq!(attachments[0]["id"], id.as_str(), "the owner's order");
    for a in attachments {
        let line = format!(
            "attachment: {} · {} · {} · {} bytes · SHA-256 {}",
            a["id"].as_str().unwrap(),
            a["kind"].as_str().unwrap(),
            a["name"].as_str().unwrap(),
            a["size_bytes"],
            a["sha256"].as_str().unwrap()
        );
        assert!(out.contains(&line), "{line}\n{out}");
    }
    assert!(out.contains(NOT_REPORTED), "{out}");
    assert_eq!(fake.tell_count(), 1);
}

#[test]
fn ad13_preview_and_check_take_no_attachment() {
    let fake = SendFake::new("ad13_read_only", "send");
    for args in [
        vec![CLOUD, "--preview", "--attachment", "att-syn-0001"],
        vec![
            CLOUD,
            "--op-id",
            "op-x",
            "--check",
            "--attach",
            "/synthetic/x.txt",
        ],
    ] {
        let (code, out, err) = fake.cli(&args, "");
        assert_eq!(code, 2, "{args:?}: {out}{err}");
        assert!(err.contains("never sends"), "{err}");
    }
    assert!(fake.invocations().is_empty());
}

/// Defense in depth for AD5: the adapter itself never starts a steer that
/// carries attachments, whoever calls it.
#[test]
fn ad05_the_adapter_never_starts_a_steer_with_attachments() {
    use openwiz_core::attachment::{Kind, SafeName, Sha256, StagedAttachment, StagedPath};
    use openwiz_core::messaging::send::{Messenger, TellReply};
    use openwiz_core::prompt::ReadyPrompt;
    let fake = SendFake::new("ad05_adapter", "send");
    let id = staging::attachment_id("att-syn-0001");
    let name = SafeName::parse("syn-notes.txt").unwrap();
    let staged = StagedAttachment {
        path: StagedPath::new(
            std::path::Path::new("/synthetic/state/attachments"),
            &id,
            &name,
        )
        .unwrap(),
        attachment: PromptAttachment {
            id,
            sha256: Sha256::parse(&"0".repeat(64)).unwrap(),
            size_bytes: 1,
            kind: Kind::File,
            name,
        },
    };
    let prompt = ReadyPrompt {
        text: "Synthetic".to_owned(),
        attachments: vec![staged],
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let reply = fake
        .source()
        .tell_prompt(&sid(CLOUD), Mode::Steer, &prompt, deadline);
    assert_eq!(reply, TellReply::NotStarted);
    assert!(fake.invocations().is_empty(), "nothing was run");
}

/// the check through the real binary: `openwiz attachment ls` and `rm` judge an
/// attachment by its operation's record, as `send` and `new` do. An
/// uncertain send's attachment is listed as kept while its operation may
/// use it (no expiry), and can't be removed.
#[test]
fn ad09_attachment_ls_and_rm_see_the_claim_through_the_release_binary() {
    let fake = SendFake::with(
        "ad09_cli_claims",
        vec![tell(
            CLOUD,
            "tell-sent.json",
            json!({ "kill_parent": true }),
        )],
    );
    let now = staging::now();
    let a = stage(&fake.state, "syn-ad09-cli.txt", &text("a\n"), now);
    let id = id_of(&a);
    let killed = fake.run_process(
        &[
            "send",
            CLOUD,
            "--op-id",
            "op-ad09-cli",
            "--attachment",
            &id,
            "--",
            "Synthetic",
        ],
        "",
    );
    assert_eq!(
        killed.status.code(),
        None,
        "killed during the call: uncertain"
    );
    let (code, listed) = json_of(&fake.run_process(&["attachment", "ls", "--json"], ""));
    assert_eq!(code, Some(0), "{listed}");
    let entry = listed["attachments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["attachment"]["id"] == id.as_str())
        .unwrap_or_else(|| panic!("not listed: {listed}"));
    assert_eq!(
        entry["expires_at"],
        Value::Null,
        "kept while uncertain: {entry}"
    );
    let removed = fake.run_process(&["attachment", "rm", &id], "");
    assert_eq!(
        removed.status.code(),
        Some(14),
        "{}",
        String::from_utf8_lossy(&removed.stdout)
    );
    assert!(
        kept(&fake.state, &a),
        "an uncertain send's attachment is kept"
    );
}

#[test]
fn ad07_legacy_queued_receipt_keeps_staged_copies_and_prompt() {
    let fake = SendFake::with(
        "ad07_legacy_queue",
        vec![tell(LOCAL, "tell-queued-thread-busy.json", json!({}))],
    );
    let a = stage(&fake.state, "syn-legacy-a.txt", &text("Synthetic a"), T0);
    let b = stage(&fake.state, "syn-legacy-b.png", &png(1024), T0);
    let r = with_ids(
        request(LOCAL, Mode::Queue, "Synthetic legacy", "op-ad07-legacy"),
        &[&a, &b],
    );
    let report = report_with(&fake, &r);
    assert_eq!(report.status, Status::Uncertain);
    assert!(kept(&fake.state, &a));
    assert!(kept(&fake.state, &b));
    assert_eq!(fake.record("op-ad07-legacy")["text"], "Synthetic legacy");
    assert_eq!(fake.tell_count(), 1);
}
