//! M5 Effects are worked out before sending, with the read's age, for every
//! state. A queue that appears paused is shown before anything is sent, and
//! the CLI does not send without the owner's acknowledgement. Unknowns stay
//! unknown.

use openwiz_core::messaging::Mode;
use openwiz_core::messaging::effects::{
    Activity, EffectKind, Effects, QueuedWork, TargetRecord, work_out,
};
use openwiz_core::messaging::send::Status;
use openwiz_core::model::{Fact, SessionId, Timestamp};
use openwiz_core::source::Placement;

use crate::support::*;

fn effects_of(fake: &SendFake, target: &str, mode: Mode, op_id: &str) -> Effects {
    let mut r = request(target, mode, "Synthetic", op_id);
    r.resume_paused_queue = true;
    r.accept_possible_duplicate = true;
    fake.report(&r)
        .effects
        .unwrap_or_else(|| panic!("{target}: no effects"))
}

fn kind(effects: &Effects) -> Option<EffectKind> {
    match &effects.effect {
        Fact::Known(effect) => Some(effect.kind),
        Fact::Unknown(_) => None,
    }
}

#[test]
fn effects_for_every_state_read_through_the_adapter() {
    let fake = SendFake::new("m05_every_state", "send");
    let cases = [
        // idle cloud
        (
            "thr_syn_0001",
            Mode::Queue,
            Some(EffectKind::StartsTurn),
            false,
        ),
        // active cloud
        (
            "thr_syn_0006",
            Mode::Queue,
            Some(EffectKind::QueuesBehindTurn),
            false,
        ),
        (
            "thr_syn_0006",
            Mode::Steer,
            Some(EffectKind::AsksToSteer),
            false,
        ),
        // active local, host connected
        (
            "thr_syn_0002",
            Mode::Queue,
            Some(EffectKind::QueuesBehindTurn),
            false,
        ),
        // a pending interaction
        (
            "thr_syn_0012",
            Mode::Queue,
            Some(EffectKind::WaitsForInteraction),
            false,
        ),
        (
            "thr_syn_0012",
            Mode::Steer,
            Some(EffectKind::WaitsForInteraction),
            false,
        ),
        // a local host missing from the machine list: unknown
        ("thr_syn_0004", Mode::Queue, None, false),
        // an error status: unknown
        ("thr_syn_0003", Mode::Queue, None, false),
        // an unrecognised status: unknown
        ("thr_syn_0014", Mode::Queue, None, false),
        // missing from thread list: unknown, and a paused queue cannot be
        // ruled out
        ("thr_syn_0015", Mode::Queue, None, true),
        // idle with queued work waiting: appears paused
        (
            "thr_syn_0011",
            Mode::Queue,
            Some(EffectKind::StartsTurn),
            true,
        ),
    ];
    for (i, (target, mode, expected, ack)) in cases.into_iter().enumerate() {
        let effects = effects_of(&fake, target, mode, &format!("op-m05-{i}"));
        assert_eq!(kind(&effects), expected, "{target} {mode:?}: {effects:#?}");
        assert_eq!(effects.needs_acknowledgement, ack, "{target}: {effects:#?}");
        match (&effects.effect, expected) {
            (Fact::Known(effect), Some(kind)) => assert_eq!(effect.text, kind.words()),
            (Fact::Unknown(why), None) => assert!(!why.trim().is_empty()),
            other => panic!("{target}: {other:?}"),
        }
        assert_eq!(effects.read_at, T0, "the read's time");
        assert_eq!(effects.read_age_ms, 0, "the fixed clock does not move");
    }
    let words = effects_of(&fake, "thr_syn_0001", Mode::Queue, "op-m05-words");
    assert_eq!(
        words.effect,
        Fact::Known(openwiz_core::messaging::effects::Effect {
            kind: EffectKind::StartsTurn,
            text: "starts a turn; may wake it".to_owned()
        })
    );
    let paused = effects_of(&fake, "thr_syn_0011", Mode::Queue, "op-m05-paused");
    match &paused.paused_queue {
        Fact::Known(p) => {
            assert!(p.appears_paused);
            assert_eq!(p.queued_messages, Fact::Known(2));
            assert_eq!(
                p.text,
                "resumes the paused queue; 2 queued messages may run after this one"
            );
        }
        other => panic!("{other:?}"),
    }
    let missing = effects_of(&fake, "thr_syn_0015", Mode::Queue, "op-m05-missing");
    assert!(
        matches!(missing.paused_queue, Fact::Unknown(_)),
        "{missing:#?}"
    );
}

fn target() -> TargetRecord {
    TargetRecord {
        id: SessionId("thr_syn_0001".to_owned()),
        title: Fact::Known("Synthetic session 1".to_owned()),
        activity: Fact::Known(Activity::Idle),
        hidden: Fact::Known(false),
        archived: Fact::Known(false),
        placement: Fact::Known(Placement::Cloud),
        host_connected: None,
        queued_work: Fact::Known(QueuedWork::None),
        queued_message_count: Fact::Known(0),
        pending_interaction: Fact::Known(false),
        problems: vec![],
    }
}

#[test]
fn the_core_rule_covers_states_the_fixtures_cannot_show() {
    let read = Timestamp(1_000_000);
    let now = Timestamp(1_004_500);
    let base = work_out(&target(), Mode::Queue, read, now);
    assert_eq!(base.read_age_ms, 4_500, "the read's age when shown");
    assert_eq!(kind(&base), Some(EffectKind::StartsTurn));

    let mut offline = target();
    offline.placement = Fact::Known(Placement::Host("mch_syn_0001".to_owned()));
    offline.host_connected = Some(Fact::Known(false));
    let e = work_out(&offline, Mode::Steer, read, now);
    assert_eq!(kind(&e), Some(EffectKind::WaitsForHost));
    assert_eq!(
        EffectKind::WaitsForHost.words(),
        "waits for its host to come back online"
    );

    let mut host_unknown = offline.clone();
    host_unknown.host_connected = Some(Fact::Unknown("synthetic".to_owned()));
    assert_eq!(kind(&work_out(&host_unknown, Mode::Queue, read, now)), None);

    // Queued work waiting behind a running turn is not a paused queue.
    let mut running = target();
    running.activity = Fact::Known(Activity::Running);
    running.queued_work = Fact::Known(QueuedWork::Waiting);
    running.queued_message_count = Fact::Known(3);
    let e = work_out(&running, Mode::Queue, read, now);
    assert!(!e.needs_acknowledgement, "{e:#?}");
    assert_eq!(kind(&e), Some(EffectKind::QueuesBehindTurn));

    // Waiting work on a thread whose activity is unknown: cannot be ruled out.
    let mut unknown_activity = running.clone();
    unknown_activity.activity = Fact::Unknown("synthetic".to_owned());
    let e = work_out(&unknown_activity, Mode::Queue, read, now);
    assert!(e.needs_acknowledgement, "{e:#?}");
    assert!(matches!(e.paused_queue, Fact::Unknown(_)));
    assert_eq!(kind(&e), None);

    // Unknown queued work, but nothing queued: no paused queue to resume.
    let mut nothing_queued = target();
    nothing_queued.queued_work = Fact::Unknown("synthetic".to_owned());
    let e = work_out(&nothing_queued, Mode::Queue, read, now);
    assert!(!e.needs_acknowledgement, "{e:#?}");

    // An unknown count on a paused queue still names the effect.
    let mut paused = target();
    paused.queued_work = Fact::Known(QueuedWork::Waiting);
    paused.queued_message_count = Fact::Unknown("synthetic".to_owned());
    let e = work_out(&paused, Mode::Queue, read, now);
    assert!(e.needs_acknowledgement);
    match &e.paused_queue {
        Fact::Known(p) => assert_eq!(
            p.text,
            "resumes the paused queue; an unknown number of queued messages may run after this one"
        ),
        other => panic!("{other:?}"),
    }

    // A pending interaction that cannot be read: unknown, never guessed.
    let mut interaction = target();
    interaction.pending_interaction = Fact::Unknown("synthetic".to_owned());
    assert_eq!(kind(&work_out(&interaction, Mode::Queue, read, now)), None);
}

#[test]
fn effects_are_worked_out_and_recorded_before_the_call() {
    let fake = SendFake::new("m05_before_the_call", "send");
    let report = fake.report(&request(
        "thr_syn_0006",
        Mode::Queue,
        "Synthetic",
        "op-m05-before",
    ));
    let at_tell = fake.records_at_tell(1);
    assert_eq!(at_tell.len(), 1, "{at_tell:#?}");
    let on_disk = &at_tell[0];
    assert_eq!(on_disk["state"], "dispatching");
    assert_eq!(
        on_disk["effects"],
        serde_json::to_value(report.effects.as_ref().expect("effects")).unwrap(),
        "the effects shown are the ones on disk before the call"
    );
    assert_eq!(
        on_disk["effects"]["effect"]["known"]["text"],
        "queues behind the current turn"
    );
}

#[test]
fn a_paused_queue_is_shown_and_not_sent_without_acknowledgement() {
    let fake = SendFake::new("m05_paused_gate", "send");
    let (code, out, err) = fake.cli(&["thr_syn_0011", "--", "Synthetic"], "");
    assert_eq!(code, 5, "{out}{err}");
    assert!(
        out.contains("resumes the paused queue; 2 queued messages may run after this one"),
        "{out}"
    );
    assert!(out.contains("--resume-paused-queue"), "{out}");
    assert_eq!(fake.tell_count(), 0, "nothing sent");

    let (code, json) = fake.cli_json(&["thr_syn_0011", "--", "Synthetic again"], "");
    assert_eq!(code, 5);
    assert_eq!(json["status"], "failed");
    assert_eq!(
        json["result"]["delivery"]["reason"],
        "paused_queue_not_acknowledged"
    );
    assert_eq!(json["effects"]["needs_acknowledgement"], true);
    assert_eq!(fake.tell_count(), 0);

    let (code, out, err) = fake.cli(
        &["thr_syn_0011", "--resume-paused-queue", "--", "Synthetic"],
        "",
    );
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("resumes the paused queue"), "{out}");
    assert_eq!(fake.tell_count(), 1);
}

#[test]
fn unknowns_stay_unknown_in_json_and_text() {
    let fake = SendFake::new("m05_unknowns", "send");
    let (code, json) = fake.cli_json(&["thr_syn_0014", "--", "Synthetic"], "");
    assert_eq!(code, 0, "{json}");
    let why = json["effects"]["effect"]["unknown"]
        .as_str()
        .expect("an unknown effect");
    assert!(!why.is_empty());
    let (_, text, _) = fake.cli(
        &[
            "thr_syn_0014",
            "--accept-possible-duplicate",
            "--",
            "Synthetic two",
        ],
        "",
    );
    assert!(text.contains(&format!("unknown ({why})")), "{text}");
    assert_eq!(fake.tell_count(), 2);
    let _ = Status::Accepted;
}

#[test]
fn the_reads_age_counts_from_the_start_of_the_target_read() {
    use openwiz_cli::SystemClock;
    use openwiz_core::messaging::send::send;
    let fake = SendFake::with(
        "m05_read_age",
        vec![serde_json::json!({
            "argv": ["thread", "list", "--json"],
            "delay_ms": 1500,
            "stdout_file": send_fixtures().join("thread-list.json").to_str().unwrap()
        })],
    );
    let report = send(
        &fake.source(),
        &fake.store(),
        &SystemClock,
        &request("thr_syn_0001", Mode::Queue, "Synthetic", "op-m05-age"),
    )
    .unwrap();
    let effects = report.effects.expect("effects");
    assert!(
        effects.read_age_ms >= 1_400,
        "the age hides the slow read: {effects:#?}"
    );
}
