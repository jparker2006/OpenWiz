//! What a send is expected to do, worked out before sending from one read of
//! the target, and shown with that read's age.
//!
//! These are expectations from a read, not observations of the send. The
//! thread can change between the read and the dispatch: an idle thread can
//! become busy, a steer can end up queued, someone can pause or resume the
//! queue, and the thread can fail or be archived. OpenWiz only guarantees
//! that it requests the chosen mode and no other, that it reports the
//! requested mode and the provider's answer separately, and that it never
//! claims an effect it did not observe.

use serde::{Deserialize, Serialize};

use crate::model::{Fact, SessionId, Timestamp};
use crate::source::Placement;

use super::Mode;
use super::send::SendGap;

/// What the target is doing, as its status reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    /// Not running a turn.
    Idle,
    /// Running a turn.
    Running,
    /// Reports an error.
    Errored,
}

/// The target's queued work, as its list row reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueuedWork {
    None,
    /// Queued work is waiting. On a thread that is not running, this is the
    /// nearest documented sign of a paused queue.
    Waiting,
}

/// The target as read just before sending. The adapter fills it from its
/// allowlisted reads; anything it could not read is unknown with a reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetRecord {
    pub id: SessionId,
    pub title: Fact<String>,
    pub activity: Fact<Activity>,
    pub hidden: Fact<bool>,
    pub archived: Fact<bool>,
    pub placement: Fact<Placement>,
    /// For a local session, whether its host is connected. `None` for a
    /// cloud session, or when the placement is unknown.
    pub host_connected: Option<Fact<bool>>,
    pub queued_work: Fact<QueuedWork>,
    pub queued_message_count: Fact<u64>,
    pub pending_interaction: Fact<bool>,
    /// Fields that could not be read, in OpenWiz's own words.
    pub problems: Vec<SendGap>,
}

/// The effect a send is expected to have, in the core's words.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectKind {
    /// Idle: "starts a turn; may wake it".
    StartsTurn,
    /// Busy, queue: "queues behind the current turn".
    QueuesBehindTurn,
    /// Busy, steer: "asks to steer the current turn".
    AsksToSteer,
    /// A pending interaction: "waits for …".
    WaitsForInteraction,
    /// A local host that is not connected: "waits for …".
    WaitsForHost,
}

impl EffectKind {
    pub fn words(self) -> &'static str {
        match self {
            Self::StartsTurn => "starts a turn; may wake it",
            Self::QueuesBehindTurn => "queues behind the current turn",
            Self::AsksToSteer => "asks to steer the current turn",
            Self::WaitsForInteraction => "waits for the pending interaction to be answered",
            Self::WaitsForHost => "waits for its host to come back online",
        }
    }
}

/// An effect and its words.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Effect {
    pub kind: EffectKind,
    pub text: String,
}

/// Whether the target's queue appears paused. No paused flag is documented
/// for `room-cli` 0.43.1, so this is always an inference.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PausedQueue {
    pub appears_paused: bool,
    /// Messages already queued, which may run after this one.
    pub queued_messages: Fact<u64>,
    /// "no paused queue seen", or "resumes the paused queue; N queued
    /// messages may run after this one".
    pub text: String,
}

/// The effects of one send, worked out before sending.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Effects {
    /// When the target was read.
    pub read_at: Timestamp,
    /// The read's age when the effects were shown, just before the dispatch.
    pub read_age_ms: i64,
    pub effect: Fact<Effect>,
    pub paused_queue: Fact<PausedQueue>,
    /// True when the queue appears paused, or that cannot be ruled out: the
    /// owner must acknowledge resuming it before anything is sent.
    pub needs_acknowledgement: bool,
}

/// Works out the effects of sending in `mode` to `target`, read at
/// `read_at`, as shown at `now`.
pub fn work_out(target: &TargetRecord, mode: Mode, read_at: Timestamp, now: Timestamp) -> Effects {
    let paused_queue = paused_queue(target);
    let needs_acknowledgement = match &paused_queue {
        Fact::Known(paused) => paused.appears_paused,
        Fact::Unknown(_) => true,
    };
    Effects {
        read_at,
        read_age_ms: now.0.saturating_sub(read_at.0),
        effect: effect(target, mode),
        paused_queue,
        needs_acknowledgement,
    }
}

/// The text when no paused queue is seen.
pub const NO_PAUSED_QUEUE: &str = "no paused queue seen";

fn effect(target: &TargetRecord, mode: Mode) -> Fact<Effect> {
    let known = |kind: EffectKind| {
        Fact::Known(Effect {
            kind,
            text: kind.words().to_owned(),
        })
    };
    match &target.pending_interaction {
        Fact::Known(true) => return known(EffectKind::WaitsForInteraction),
        Fact::Known(false) => {}
        Fact::Unknown(why) => {
            return Fact::Unknown(format!("whether an interaction is pending: {why}"));
        }
    }
    match (&target.placement, &target.host_connected) {
        (_, Some(Fact::Known(false))) => return known(EffectKind::WaitsForHost),
        (_, Some(Fact::Unknown(why))) => {
            return Fact::Unknown(format!("whether its host is connected: {why}"));
        }
        (Fact::Unknown(why), None) => {
            return Fact::Unknown(format!("where it runs: {why}"));
        }
        _ => {}
    }
    match &target.activity {
        Fact::Known(Activity::Idle) => known(EffectKind::StartsTurn),
        Fact::Known(Activity::Running) => known(match mode {
            Mode::Queue => EffectKind::QueuesBehindTurn,
            Mode::Steer => EffectKind::AsksToSteer,
        }),
        Fact::Known(Activity::Errored) => Fact::Unknown(
            "the thread reports an error; how it takes a new message is not documented".to_owned(),
        ),
        Fact::Unknown(why) => Fact::Unknown(format!("its status: {why}")),
    }
}

fn paused_queue(target: &TargetRecord) -> Fact<PausedQueue> {
    let not_paused = || {
        Fact::Known(PausedQueue {
            appears_paused: false,
            queued_messages: target.queued_message_count.clone(),
            text: NO_PAUSED_QUEUE.to_owned(),
        })
    };
    match (&target.queued_work, &target.activity) {
        (Fact::Known(QueuedWork::Waiting), Fact::Known(Activity::Idle)) => {
            let follow = match &target.queued_message_count {
                Fact::Known(1) => "1 queued message may run after this one".to_owned(),
                Fact::Known(n) => format!("{n} queued messages may run after this one"),
                Fact::Unknown(_) => {
                    "an unknown number of queued messages may run after this one".to_owned()
                }
            };
            Fact::Known(PausedQueue {
                appears_paused: true,
                queued_messages: target.queued_message_count.clone(),
                text: format!("resumes the paused queue; {follow}"),
            })
        }
        // Waiting behind a running turn: the queue is moving.
        (Fact::Known(QueuedWork::Waiting), Fact::Known(Activity::Running)) => not_paused(),
        (Fact::Known(QueuedWork::Waiting), Fact::Known(Activity::Errored)) => {
            Fact::Unknown("queued work is waiting on a thread that reports an error".to_owned())
        }
        (Fact::Known(QueuedWork::Waiting), Fact::Unknown(why)) => Fact::Unknown(format!(
            "queued work is waiting, and its status is unknown: {why}"
        )),
        (Fact::Known(QueuedWork::None), _) => not_paused(),
        (Fact::Unknown(why), _) => match target.queued_message_count {
            // Nothing is queued, so there is nothing a send could resume.
            Fact::Known(0) => not_paused(),
            _ => Fact::Unknown(format!("its queued work: {why}")),
        },
    }
}
