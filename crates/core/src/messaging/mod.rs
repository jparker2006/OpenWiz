//! Messaging: OpenWiz's one write, `send`. It delivers one text message to
//! one existing session and reports what the provider said.
//!
//! The **requested mode** (queue or steer) and the **reported delivery**
//! (queued, accepted, failed or uncertain) are kept apart everywhere: the
//! requested mode is never shown as what happened.
//!
//! Like the rest of the core, this module does no I/O. Reads and the one
//! write go through a [`send::Messenger`] (the adapter), the durable send
//! record through a [`record::SendStore`] (`openwiz-store`), and time through
//! a [`Clock`](crate::ops::Clock). Op ids are generated outside the core.
//!
//! - [`send`]: the operation, its outcomes and its report;
//! - [`preview`]: what a send would do, read before any text is written,
//!   with no claim and no call;
//! - [`lookup`]: one op id's record, for "Check again", with no call;
//! - [`effects`]: what a send is expected to do, worked out before sending;
//! - [`record`]: the send record, the store interface and the recovery rule;
//! - [`reconcile`]: the rule for a lost acknowledgement.

pub mod effects;
pub mod lookup;
pub mod preview;
pub mod reconcile;
pub mod record;
pub mod send;
mod sha256;

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::model::SessionId;

pub use sha256::{Sha256Hasher, sha256_hex};

/// The schema id of a send report. See `contracts/messaging.v1.schema.json`.
pub const MESSAGING_SCHEMA_ID: &str = "openwiz.messaging.v1";

/// The schema id of a send record.
pub const RECORD_SCHEMA_ID: &str = "openwiz.send-record.v1";

/// The largest message, in bytes of UTF-8. Provisional ; see
/// "Provisional choices for the maintainer" in ticket 0.0.3-1's evidence. The text is
/// one command-line argument, so the cap stays far below every Linux and
/// macOS argument limit: Linux allows 128 KiB for one argument
/// (`MAX_ARG_STRLEN`, 32 pages) and macOS 1 MiB for all of them (`ARG_MAX`).
pub const MAX_TEXT_BYTES: usize = 16 * 1024;

/// The longest op id.
pub const OP_ID_MAX_LEN: usize = 64;

/// One send, from the claim to the recorded outcome, finishes within this
/// budget. The adapter also stops each single call after
/// [`READ_TIMEOUT`](crate::limits::READ_TIMEOUT).
pub const SEND_BUDGET: Duration = Duration::from_secs(30);

/// The mode OpenWiz asks for. Always passed explicitly: never `auto`, and
/// never the provider's location default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// The default: wait behind the current turn.
    #[default]
    Queue,
    /// Only when asked for: steer the current turn.
    Steer,
}

impl Mode {
    /// The word passed to the provider: `queue` or `steer`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queue => "queue",
            Self::Steer => "steer",
        }
    }

    /// `Queue requested` or `Steer requested`: what was asked, never what
    /// happened.
    pub fn requested(self) -> &'static str {
        match self {
            Self::Queue => "Queue requested",
            Self::Steer => "Steer requested",
        }
    }
}

/// An operation id: the owner's handle on one send. One op id makes at most
/// one provider call, ever, and is bound to one target, mode and text.
///
/// 1 to [`OP_ID_MAX_LEN`] ASCII letters, digits, `_` and `-`, starting with
/// a letter or digit, so it is also a safe file name.
///
/// It is checked however it arrives: deserializing goes through
/// [`OpId::parse`] too, so a record on disk cannot carry an invalid one.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct OpId(String);

impl TryFrom<String> for OpId {
    type Error = InvalidRequest;

    fn try_from(id: String) -> Result<Self, Self::Error> {
        Self::parse(&id)
    }
}

impl From<OpId> for String {
    fn from(id: OpId) -> Self {
        id.0
    }
}

impl OpId {
    pub fn parse(id: &str) -> Result<Self, InvalidRequest> {
        let valid = !id.is_empty()
            && id.len() <= OP_ID_MAX_LEN
            && id.as_bytes()[0].is_ascii_alphanumeric()
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        if valid {
            Ok(Self(id.to_owned()))
        } else {
            Err(InvalidRequest(format!(
                "an op id is 1 to {OP_ID_MAX_LEN} ASCII letters, digits, _ or -, \
                 starting with a letter or digit"
            )))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OpId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why a request was refused before anything was claimed, read or sent.
/// The wording is OpenWiz's own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidRequest(pub String);

impl fmt::Display for InvalidRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Text only: not empty, not only whitespace, no NUL, and at most
/// [`MAX_TEXT_BYTES`] bytes of UTF-8. Everything else, a leading `-`, line
/// breaks, any Unicode or a file path, is sent as it is.
pub fn validate_text(text: &str) -> Result<(), InvalidRequest> {
    let refuse = |why: &str| Err(InvalidRequest(why.to_owned()));
    if text.is_empty() {
        return refuse("the text is empty");
    }
    if text.trim().is_empty() {
        return refuse("the text is only whitespace");
    }
    if text.contains('\0') {
        return refuse("the text contains a NUL character");
    }
    if text.len() > MAX_TEXT_BYTES {
        return Err(InvalidRequest(format!(
            "the text is {} bytes; the cap is {MAX_TEXT_BYTES} bytes of UTF-8",
            text.len()
        )));
    }
    Ok(())
}

/// What an op id is bound to: the target, the mode, a SHA-256 of the text
/// and the attachments digest (B2). The text itself is not part of the
/// binding.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub target: SessionId,
    pub mode: Mode,
    /// Lowercase hex SHA-256 of the text's UTF-8 bytes.
    pub text_sha256: String,
    /// [`attachments_digest`](crate::prompt::attachments_digest) of the
    /// attachments, in the owner's order. **Absent means none** (B2): it is
    /// written only when there are attachments, so every 0.0.3 record and
    /// every text-only send keeps its shape.
    #[serde(
        default = "crate::prompt::empty_attachments_digest",
        skip_serializing_if = "is_empty_digest"
    )]
    pub attachments_digest: String,
}

fn is_empty_digest(digest: &str) -> bool {
    digest == crate::prompt::empty_attachments_digest()
}

/// A part of a binding, named when a reused op id differs in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingField {
    Target,
    Mode,
    Text,
    /// The attachments: added, removed, reordered or with other bytes (B2).
    Attachments,
}

impl Binding {
    /// A text-only binding: no attachments.
    pub fn new(target: SessionId, mode: Mode, text: &str) -> Self {
        Self {
            target,
            mode,
            text_sha256: sha256_hex(text.as_bytes()),
            attachments_digest: crate::prompt::empty_attachments_digest(),
        }
    }

    /// The binding of a send of `text` with attachments whose digest is
    /// `attachments_digest`.
    pub fn with_attachments(
        target: SessionId,
        mode: Mode,
        text: &str,
        attachments_digest: String,
    ) -> Self {
        Self {
            attachments_digest,
            ..Self::new(target, mode, text)
        }
    }

    /// The fields in which `other` differs from this binding.
    pub fn differences(&self, other: &Binding) -> Vec<BindingField> {
        let mut differs = Vec::new();
        if self.target != other.target {
            differs.push(BindingField::Target);
        }
        if self.mode != other.mode {
            differs.push(BindingField::Mode);
        }
        if self.text_sha256 != other.text_sha256 {
            differs.push(BindingField::Text);
        }
        if self.attachments_digest != other.attachments_digest {
            differs.push(BindingField::Attachments);
        }
        differs
    }
}
