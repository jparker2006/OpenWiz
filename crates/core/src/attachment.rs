//! Attachments: OpenWiz 0.0.4's file model (B1 phase 1).
//!
//! One attachment is one file the owner chose, copied unchanged to the
//! controller. This module holds the rules, with no I/O: the store (phase
//! 2) reads, copies and hashes, and asks the model what the bytes are and
//! what may happen next.
//!
//! - **Kind from the bytes** ([`classify`]): PNG, JPEG, GIF and WebP are
//! images, by their signature. HEIC and HEIF are refused, as is an ISO
//! media file whose brands don't settle it. Everything else is a file,
//! sent unchanged and never interpreted (a ZIP is never unpacked). A
//! declared MIME type has no way in.
//! - **Name and content agree**: CloudRoom types a file by its
//! extension, so an image under another image's name, or a
//! non-image under an image's name, is refused. Nothing is renamed.
//! - **Safe names** ([`SafeName`]): the staged name, which CloudRoom's own
//! sanitizing leaves unchanged (F5). CloudRoom then stores it as
//! `<stem>-<time>-<random><ext>`, and that stored name is what the agent
//! sees; the staged limit leaves room for the suffix.
//! - **Provider limits only** ([`limit`]): 10 MiB per image and 25 MiB per
//! file (F7), and the image limit for a file CloudRoom types as an image
//! (F26, [`IMAGE_TYPED_EXTENSIONS`]). No count cap, no total.
//! - **Readiness, retention and the free-disk floor** ([`Readiness`],
//! [`retention`], [`staging_room`]): shared readiness, retention and free-disk rules.
//!
//! CloudRoom's limits and typing are source-read, not observed; the live
//! check rechecks them (AL2). Nothing here dispatches anything.

use std::fmt;
use std::time::Duration;

use serde::de::{self, MapAccess, Unexpected, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The schema id of the attachment contract. See
/// `contracts/attachment.v1.schema.json`.
pub const ATTACHMENT_SCHEMA_ID: &str = "openwiz.attachment.v1";

/// CloudRoom's limit per image (F7). Source-read; rechecked live (AL2).
pub const IMAGE_MAX_BYTES: u64 = 10 * 1024 * 1024;

/// CloudRoom's limit per file (F7). Source-read; rechecked live (AL2).
pub const FILE_MAX_BYTES: u64 = 25 * 1024 * 1024;

/// The Cloud core's longest attachment name, in bytes (F5).
pub const CLOUDROOM_NAME_MAX_BYTES: usize = 200;

/// The longest suffix CloudRoom adds before the extension when it stores an
/// upload: `-`, `Date.now()` (13 digits until 2286), `-`, and up to six
/// base-36 characters (`buildStoredFilename`). Source-read at upstream
/// `eb20d1c`, not observed.
pub const CLOUDROOM_SUFFIX_MAX_BYTES: usize = 21;

/// The longest staged name: CloudRoom's stored name, the staged name plus
/// its suffix, then still fits [`CLOUDROOM_NAME_MAX_BYTES`].
pub const SAFE_NAME_MAX_BYTES: usize = CLOUDROOM_NAME_MAX_BYTES - CLOUDROOM_SUFFIX_MAX_BYTES;

/// The longest attachment id.
pub const ATTACHMENT_ID_MAX_LEN: usize = 64;

/// How many leading bytes [`classify`] looks at: the file's first
/// `SNIFF_BYTES`, or the whole file if it is shorter. Every signature fits,
/// and an ISO media `ftyp` box of up to 1,000 brands; a longer one is
/// refused as unclear.
pub const SNIFF_BYTES: usize = 4096;

/// How long a ready attachment no operation has claimed is kept.
pub const UNSENT_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Staging never leaves less than this free on the state volume. A
/// safe-resource floor, not a product cap; rechecked at the live check.
pub const FREE_DISK_FLOOR_BYTES: u64 = 1024 * 1024 * 1024;

/// The extensions CloudRoom types as `image/*`, so a file with one gets the
/// image limit even when sent as a file (F26). CloudRoom looks the type up
/// with `mime-types`; its version isn't known, so this is the union of what
/// 2.1.35 (`mime-db` 1.52.0) and 3.0.2 (`mime-db` 1.54.0) look up as
/// `image/*`, recorded in the release checks. The union
/// refuses before any upload rather than part way through one (F15).
/// Sorted, lowercase, for [`slice::binary_search`].
pub const IMAGE_TYPED_EXTENSIONS: &[&str] = &[
    "3ds", "apng", "avci", "avcs", "avif", "azv", "b16", "bmp", "btf", "btif", "cgm", "cmx", "dds",
    "dib", "djv", "djvu", "dng", "dpx", "drle", "dwg", "dxf", "emf", "exr", "fbs", "fh", "fh4",
    "fh5", "fh7", "fhc", "fits", "fpx", "fst", "g3", "gif", "heic", "heics", "heif", "heifs",
    "hej2", "hsj2", "ico", "ief", "jaii", "jais", "jfif", "jhc", "jls", "jng", "jp2", "jpe",
    "jpeg", "jpf", "jpg", "jpg2", "jpgm", "jph", "jpm", "jpx", "jxl", "jxr", "jxra", "jxrs", "jxs",
    "jxsc", "jxsi", "jxss", "ktx", "ktx2", "mdi", "mmr", "npx", "pbm", "pct", "pcx", "pgm", "pic",
    "png", "pnm", "ppm", "psd", "pti", "ras", "rgb", "rlc", "sgi", "sid", "svg", "svgz", "t38",
    "tap", "tfx", "tga", "tif", "tiff", "uvg", "uvi", "uvvg", "uvvi", "vtf", "wbmp", "wdp", "webp",
    "wmf", "xbm", "xif", "xpm", "xwd",
];

/// What CloudRoom is asked to attach: an image or a file. Decided from the
/// bytes, never from a MIME type or the extension alone. In JSON, one
/// string from [`Kind::labels`], nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// PNG, JPEG, GIF or WebP: the image types every harness takes (F11).
    Image,
    /// Anything else, sent unchanged and never interpreted.
    File,
}

/// Why an attachment can't become ready. A fixed list. In JSON, one
/// string from [`FailReason::labels`], nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FailReason {
    /// HEIC or HEIF bytes. CloudRoom refuses them (F6); OpenWiz never
    /// converts.
    UnsupportedHeif,
    /// An ISO media file whose `ftyp` box can't be read in full, or names
    /// both HEVC and AV1 brands: it might be HEIC, so it is refused.
    TypeUnclear,
    /// The name's extension and the bytes disagree.
    ExtensionMismatch,
    /// An image over [`IMAGE_MAX_BYTES`].
    ImageTooLarge,
    /// A file over [`FILE_MAX_BYTES`].
    FileTooLarge,
    /// A file over [`IMAGE_MAX_BYTES`] whose extension CloudRoom types as
    /// an image (F26).
    ImageTypedFileTooLarge,
    /// A folder, or a link to one.
    Folder,
    /// Staging would go below [`FREE_DISK_FLOOR_BYTES`].
    LowDisk,
    /// The copy's size or hash isn't what was declared or recorded.
    SizeMismatch,
    /// The bytes stopped arriving.
    ConnectionLost,
    /// The owner's sign-in ended during the upload.
    SignedOut,
    /// The source couldn't be read.
    Unreadable,
}

trait Labels: Copy + PartialEq + 'static {
    /// Every value and its one JSON string.
    fn labels() -> &'static [(&'static str, Self)];

    fn label(self) -> &'static str {
        Self::labels()
            .iter()
            .find(|(_, value)| *value == self)
            .map(|(label, _)| *label)
            .unwrap_or_default()
    }
}

/// Reads a string from `T`'s list: [`Visitor::visit_str`] is the only
/// entry, so any other JSON value is refused.
struct LabelVisitor<T>(std::marker::PhantomData<T>);

impl<T: Labels> Visitor<'_> for LabelVisitor<T> {
    type Value = T;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let labels: Vec<&str> = T::labels().iter().map(|(label, _)| *label).collect();
        write!(f, "one of the strings {}", labels.join(", "))
    }

    fn visit_str<E: de::Error>(self, text: &str) -> Result<T, E> {
        T::labels()
            .iter()
            .find(|(label, _)| *label == text)
            .map(|(_, value)| *value)
            .ok_or_else(|| E::invalid_value(Unexpected::Str(text), &self))
    }
}

fn read_label<'de, D: Deserializer<'de>, T: Labels>(d: D) -> Result<T, D::Error> {
    d.deserialize_str(LabelVisitor(std::marker::PhantomData))
}

impl Labels for Kind {
    fn labels() -> &'static [(&'static str, Self)] {
        &[("image", Self::Image), ("file", Self::File)]
    }
}

impl Kind {
    /// Every kind and its JSON string.
    pub fn labels() -> &'static [(&'static str, Self)] {
        <Self as Labels>::labels()
    }
}

impl Serialize for Kind {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.label())
    }
}

impl<'de> Deserialize<'de> for Kind {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        read_label(d)
    }
}

impl Labels for FailReason {
    fn labels() -> &'static [(&'static str, Self)] {
        &[
            ("unsupported_heif", Self::UnsupportedHeif),
            ("type_unclear", Self::TypeUnclear),
            ("extension_mismatch", Self::ExtensionMismatch),
            ("image_too_large", Self::ImageTooLarge),
            ("file_too_large", Self::FileTooLarge),
            ("image_typed_file_too_large", Self::ImageTypedFileTooLarge),
            ("folder", Self::Folder),
            ("low_disk", Self::LowDisk),
            ("size_mismatch", Self::SizeMismatch),
            ("connection_lost", Self::ConnectionLost),
            ("signed_out", Self::SignedOut),
            ("unreadable", Self::Unreadable),
        ]
    }
}

impl Serialize for FailReason {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.label())
    }
}

impl<'de> Deserialize<'de> for FailReason {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        read_label(d)
    }
}

impl FailReason {
    /// Every reason and its JSON string.
    pub fn labels() -> &'static [(&'static str, Self)] {
        <Self as Labels>::labels()
    }

    /// The reason, in the core's words.
    pub fn message(self) -> &'static str {
        match self {
            Self::UnsupportedHeif => "HEIC/HEIF images aren't supported. Nothing was converted",
            Self::TypeUnclear => {
                "The file's first bytes don't show whether it is a HEIC/HEIF image, so it wasn't \
                 attached"
            }
            Self::ExtensionMismatch => {
                "The file's extension doesn't match its content. Nothing was renamed"
            }
            Self::ImageTooLarge => "CloudRoom accepts images up to 10 MiB",
            Self::FileTooLarge => "CloudRoom accepts files up to 25 MiB",
            Self::ImageTypedFileTooLarge => {
                "CloudRoom treats a file with this extension as an image, and accepts images \
                 up to 10 MiB"
            }
            Self::Folder => "Folders aren't supported yet",
            Self::LowDisk => "Low disk: staging would leave less than 1 GiB free on the controller",
            Self::SizeMismatch => "The copy doesn't match the size or hash expected",
            Self::ConnectionLost => "The connection was lost before the whole file arrived",
            Self::SignedOut => "Signed out before the upload finished",
            Self::Unreadable => "The file couldn't be read",
        }
    }
}

impl fmt::Display for FailReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

/// A value that isn't a valid attachment id, hash or safe name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidAttachment(pub String);

impl fmt::Display for InvalidAttachment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for InvalidAttachment {}

/// The **staged name**, handed to CloudRoom: the original's basename
/// reduced to `[A-Za-z0-9._-]`, 1 to [`SAFE_NAME_MAX_BYTES`] bytes, never
/// starting with `.` or `-` (so never `.`, `..` or an option). CloudRoom's
/// own sanitizing (F5) leaves it unchanged.
///
/// It is **not** the name the agent sees. CloudRoom stores the upload as
/// `<stem>-<Date.now()>-<random><ext>` and names the attachment after that
/// stored copy (source-read, not observed). The stem and the extension are
/// the staged name's, so CloudRoom types the copy as it types this name.
///
/// Deserializing goes through [`SafeName::parse`], so a record on disk
/// can't carry an invalid one.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SafeName(String);

impl SafeName {
    /// The safe name for an original file name, which may be anything.
    ///
    /// Only the basename is kept (after the last `/` or `\`). A basename
    /// that is empty, `.` or `..` becomes `attachment`. Every other
    /// character becomes `_`, as does each `.` or `-` at the start. Over
    /// [`SAFE_NAME_MAX_BYTES`], the stem is cut and the extension kept.
    pub fn from_original(original: &str) -> Self {
        let base = original.rsplit(['/', '\\']).next().unwrap_or_default();
        if matches!(base, "" | "." | "..") {
            return Self("attachment".to_owned());
        }
        let mut leading = true;
        let mut name: String = base
            .chars()
            .map(|c| {
                leading &= matches!(c, '.' | '-');
                if allowed(c) && !leading { c } else { '_' }
            })
            .collect();
        if name.len() > SAFE_NAME_MAX_BYTES {
            // ASCII only from here, so every byte index is a boundary.
            name = match name.rfind('.').filter(|&dot| dot > 0) {
                Some(dot) if name.len() - dot < SAFE_NAME_MAX_BYTES => {
                    let extension = &name[dot..];
                    let stem = &name[..SAFE_NAME_MAX_BYTES - extension.len()];
                    format!("{stem}{extension}")
                }
                _ => name[..SAFE_NAME_MAX_BYTES].to_owned(),
            };
        }
        Self(name)
    }

    /// Accepts a name only if it is already safe.
    pub fn parse(name: &str) -> Result<Self, InvalidAttachment> {
        let valid = !name.is_empty()
            && name.len() <= SAFE_NAME_MAX_BYTES
            && !name.starts_with(['.', '-'])
            && name.chars().all(allowed);
        if valid {
            Ok(Self(name.to_owned()))
        } else {
            Err(InvalidAttachment(format!(
                "a safe name is 1 to {SAFE_NAME_MAX_BYTES} ASCII letters, digits, ., _ or -, \
                 not starting with . or -"
            )))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The text after the last `.`, in lowercase, if there is any. This is
    /// what CloudRoom types the file by (F4).
    pub fn extension(&self) -> Option<String> {
        let (_, extension) = self.0.rsplit_once('.')?;
        (!extension.is_empty()).then(|| extension.to_ascii_lowercase())
    }
}

fn allowed(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')
}

impl TryFrom<String> for SafeName {
    type Error = InvalidAttachment;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        Self::parse(&name)
    }
}

impl From<SafeName> for String {
    fn from(name: SafeName) -> Self {
        name.0
    }
}

/// The controller's id for one staged attachment: opaque, and safe as one
/// path segment. 1 to [`ATTACHMENT_ID_MAX_LEN`] ASCII letters, digits, `_`
/// and `-`, starting with a letter or digit. Chosen outside the core.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AttachmentId(String);

impl AttachmentId {
    pub fn parse(id: &str) -> Result<Self, InvalidAttachment> {
        let valid = !id.is_empty()
            && id.len() <= ATTACHMENT_ID_MAX_LEN
            && id.as_bytes()[0].is_ascii_alphanumeric()
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        if valid {
            Ok(Self(id.to_owned()))
        } else {
            Err(InvalidAttachment(format!(
                "an attachment id is 1 to {ATTACHMENT_ID_MAX_LEN} ASCII letters, digits, _ or -, \
                 starting with a letter or digit"
            )))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for AttachmentId {
    type Error = InvalidAttachment;

    fn try_from(id: String) -> Result<Self, Self::Error> {
        Self::parse(&id)
    }
}

impl From<AttachmentId> for String {
    fn from(id: AttachmentId) -> Self {
        id.0
    }
}

/// A lowercase hex SHA-256: 64 characters of `0-9a-f`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Sha256(String);

impl Sha256 {
    pub fn parse(hex: &str) -> Result<Self, InvalidAttachment> {
        let valid = hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if valid {
            Ok(Self(hex.to_owned()))
        } else {
            Err(InvalidAttachment(
                "a SHA-256 is 64 lowercase hex characters".to_owned(),
            ))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Sha256 {
    type Error = InvalidAttachment;

    fn try_from(hex: String) -> Result<Self, Self::Error> {
        Self::parse(&hex)
    }
}

impl From<Sha256> for String {
    fn from(hex: Sha256) -> Self {
        hex.0
    }
}

/// The verified record of one ready attachment. A send or a new thread
/// carries only [`AttachmentId`]s; the core resolves each to this (B2).
///
/// Read only from a map with exactly these six keys, each once.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AttachmentRef {
    pub id: AttachmentId,
    pub sha256: Sha256,
    pub size_bytes: u64,
    pub kind: Kind,
    /// The staged name. CloudRoom stores the copy under this stem and
    /// extension, with its own suffix; the agent sees that stored name.
    pub name: SafeName,
    /// The owner's original file name: untrusted text, for display only.
    pub display_name: String,
}

/// Takes one key's value, refusing a key given twice.
fn once<'de, A: MapAccess<'de>, T: Deserialize<'de>>(
    map: &mut A,
    slot: &mut Option<T>,
    key: &'static str,
) -> Result<(), A::Error> {
    if slot.is_some() {
        return Err(de::Error::duplicate_field(key));
    }
    *slot = Some(map.next_value()?);
    Ok(())
}

/// A checked string: read as a string, then through its own `parse`.
fn checked<E: de::Error, T>(
    text: Option<String>,
    key: &'static str,
    parse: fn(&str) -> Result<T, InvalidAttachment>,
) -> Result<T, E> {
    let text = text.ok_or_else(|| E::missing_field(key))?;
    parse(&text).map_err(|e| E::custom(format!("{key}: {e}")))
}

struct RefVisitor;

impl<'de> Visitor<'de> for RefVisitor {
    type Value = AttachmentRef;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an attachment object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<AttachmentRef, A::Error> {
        let (mut id, mut sha256, mut size_bytes, mut kind, mut name, mut display_name) =
            (None, None, None, None, None, None);
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "id" => once(&mut map, &mut id, "id")?,
                "sha256" => once(&mut map, &mut sha256, "sha256")?,
                "size_bytes" => once(&mut map, &mut size_bytes, "size_bytes")?,
                "kind" => once(&mut map, &mut kind, "kind")?,
                "name" => once(&mut map, &mut name, "name")?,
                "display_name" => once(&mut map, &mut display_name, "display_name")?,
                other => {
                    let keys = &["id", "sha256", "size_bytes", "kind", "name", "display_name"];
                    return Err(de::Error::unknown_field(other, keys));
                }
            }
        }
        Ok(AttachmentRef {
            id: checked(id, "id", AttachmentId::parse)?,
            sha256: checked(sha256, "sha256", Sha256::parse)?,
            size_bytes: size_bytes.ok_or_else(|| de::Error::missing_field("size_bytes"))?,
            kind: kind.ok_or_else(|| de::Error::missing_field("kind"))?,
            name: checked(name, "name", SafeName::parse)?,
            display_name: display_name.ok_or_else(|| de::Error::missing_field("display_name"))?,
        })
    }
}

impl<'de> Deserialize<'de> for AttachmentRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_map(RefVisitor)
    }
}

/// The format the first bytes show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    Png,
    Jpeg,
    Gif,
    Webp,
    Heif,
    /// An ISO media file that might be HEIF: refused, never a file.
    Unclear,
    Other,
}

impl Format {
    fn sniff(bytes: &[u8]) -> Self {
        let head = &bytes[..bytes.len().min(SNIFF_BYTES)];
        if head.starts_with(b"\x89PNG\r\n\x1a\n") {
            Self::Png
        } else if head.starts_with(b"\xff\xd8\xff") {
            Self::Jpeg
        } else if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
            Self::Gif
        } else if head.starts_with(b"RIFF") && head.get(8..12) == Some(b"WEBP") {
            Self::Webp
        } else if head.get(4..8) == Some(b"ftyp") {
            iso_media(head)
        } else {
            Self::Other
        }
    }

    /// The extensions CloudRoom types as this format: the only names an
    /// image of it may have.
    fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Png => &["png"],
            Self::Jpeg => &["jpg", "jpeg", "jpe"],
            Self::Gif => &["gif"],
            Self::Webp => &["webp"],
            Self::Heif => &["heic", "heif", "heics", "heifs"],
            Self::Unclear | Self::Other => &[],
        }
    }
}

/// An ISO base media file (an `ftyp` box first): HEIF, or another format,
/// or unclear. The box header is read in both forms, a 32-bit size and the
/// 64-bit one (size 1), and every brand counts, major and compatible.
///
/// - A HEVC brand (`heic`, `heix`, `hevc`, …) is HEIC, unless an AV1 brand
/// (`avif`, `avis`) is there too: then it is unclear.
/// - An AV1 brand is AVIF: a file.
/// - Otherwise the generic HEIF brands (`mif1`, `msf1`, `mif2`) are HEIF.
/// - A box that doesn't fit in what was read, is too small for its
/// brands, or ends part way through a brand, is unclear.
fn iso_media(head: &[u8]) -> Format {
    let be = |bytes: &[u8]| bytes.iter().fold(0u64, |n, &b| (n << 8) | u64::from(b));
    let (header, size) = match be(&head[..4]) {
        1 => match head.get(8..16) {
            Some(large) => (16, be(large)),
            None => return Format::Unclear,
        },
        // To the end of the file, which must be all of it.
        0 if head.len() < SNIFF_BYTES => (8, head.len() as u64),
        0 => return Format::Unclear,
        size => (8, size),
    };
    let end = match usize::try_from(size) {
        Ok(end) if end >= header + 8 && end <= head.len() => end,
        _ => return Format::Unclear,
    };
    let major = &head[header..header + 4];
    // Whole 4-byte brands only: any byte left over is a cut-off brand,
    // which might be HEVC's.
    let (compatible, partial) = head[header + 8..end].as_chunks::<4>();
    if !partial.is_empty() {
        return Format::Unclear;
    }
    let brands: Vec<&[u8]> = std::iter::once(major)
        .chain(compatible.iter().map(|b| &b[..]))
        .collect();
    let any = |set: &[&[u8; 4]]| brands.iter().any(|b| set.iter().any(|s| b == s));
    let hevc = any(&[
        b"heic", b"heix", b"heim", b"heis", b"hevc", b"hevx", b"hevm", b"hevs",
    ]);
    let av1 = any(&[b"avif", b"avis"]);
    match (hevc, av1) {
        (true, true) => Format::Unclear,
        (true, false) => Format::Heif,
        (false, true) => Format::Other,
        (false, false) if any(&[b"mif1", b"msf1", b"mif2"]) => Format::Heif,
        (false, false) => Format::Other,
    }
}

/// The kind of a file named `name` whose first bytes are `head`, or why it
/// can't be attached. `head` is the file's first [`SNIFF_BYTES`] bytes, or
/// the whole file if it is shorter: a shorter `head` is taken as the whole
/// file. Passing more is fine; only the first [`SNIFF_BYTES`] count.
pub fn classify(name: &SafeName, head: &[u8]) -> Result<Kind, FailReason> {
    let format = Format::sniff(head);
    match format {
        Format::Heif => return Err(FailReason::UnsupportedHeif),
        Format::Unclear => return Err(FailReason::TypeUnclear),
        _ => {}
    }
    let extension = name.extension().unwrap_or_default();
    let named = [
        Format::Png,
        Format::Jpeg,
        Format::Gif,
        Format::Webp,
        Format::Heif,
    ]
    .into_iter()
    .find(|f| f.extensions().contains(&extension.as_str()));
    match (format, named) {
        (Format::Other, None) => Ok(Kind::File),
        (Format::Other, Some(_)) => Err(FailReason::ExtensionMismatch),
        (image, Some(named)) if named == image => Ok(Kind::Image),
        _ => Err(FailReason::ExtensionMismatch),
    }
}

/// The largest size CloudRoom accepts for an attachment, and the reason
/// given one byte over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limit {
    pub max_bytes: u64,
    pub over: FailReason,
}

impl Limit {
    pub fn admits(self, size_bytes: u64) -> Result<(), FailReason> {
        if size_bytes <= self.max_bytes {
            Ok(())
        } else {
            Err(self.over)
        }
    }
}

/// CloudRoom's limit for an attachment of `kind` named `name`: the image
/// limit for an image, or for a file whose extension CloudRoom types as an
/// image (F26); otherwise the file limit.
pub fn limit(kind: Kind, name: &SafeName) -> Limit {
    let image_typed = name
        .extension()
        .is_some_and(|e| IMAGE_TYPED_EXTENSIONS.binary_search(&e.as_str()).is_ok());
    match kind {
        Kind::Image => Limit {
            max_bytes: IMAGE_MAX_BYTES,
            over: FailReason::ImageTooLarge,
        },
        Kind::File if image_typed => Limit {
            max_bytes: IMAGE_MAX_BYTES,
            over: FailReason::ImageTypedFileTooLarge,
        },
        Kind::File => Limit {
            max_bytes: FILE_MAX_BYTES,
            over: FailReason::FileTooLarge,
        },
    }
}

/// The whole check for one attachment: its kind, then its size against
/// that kind's limit. Each attachment stands alone: nothing counts or
/// totals them.
pub fn check(name: &SafeName, head: &[u8], size_bytes: u64) -> Result<Kind, FailReason> {
    let kind = classify(name, head)?;
    limit(kind, name).admits(size_bytes)?;
    Ok(kind)
}

/// Whether `incoming_bytes` more may be staged with `free_bytes` free on
/// the state volume: never below [`FREE_DISK_FLOOR_BYTES`].
pub fn staging_room(free_bytes: u64, incoming_bytes: u64) -> Result<(), FailReason> {
    match free_bytes.checked_sub(incoming_bytes) {
        Some(left) if left >= FREE_DISK_FLOOR_BYTES => Ok(()),
        _ => Err(FailReason::LowDisk),
    }
}

/// One attachment's readiness. `selected` and `uploading` exist only
/// in clients and in an upload in flight; the store holds `ready`.
///
/// In JSON, `{"state": <state>}`, or `{"state": "failed", "reason":
/// <reason>}`: read only from a map with exactly those keys, each once,
/// and string values from their lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Readiness {
    /// Chosen in a client only. Claims no upload.
    Selected,
    /// Bytes are arriving at the controller. Claims nothing complete.
    Uploading,
    /// The whole copy is on the controller, with its size, SHA-256, kind
    /// and safe name verified. Claims nothing about CloudRoom.
    Ready,
    /// It can't become ready, for this reason.
    Failed { reason: FailReason },
    /// The owner removed it; its bytes are deleted.
    Removed,
    /// Unclaimed past [`UNSENT_TTL`]; its bytes are deleted.
    Expired,
}

/// A readiness object's `state`.
#[derive(Clone, Copy, PartialEq)]
enum State {
    Selected,
    Uploading,
    Ready,
    Failed,
    Removed,
    Expired,
}

impl Labels for State {
    fn labels() -> &'static [(&'static str, Self)] {
        &[
            ("selected", Self::Selected),
            ("uploading", Self::Uploading),
            ("ready", Self::Ready),
            ("failed", Self::Failed),
            ("removed", Self::Removed),
            ("expired", Self::Expired),
        ]
    }
}

impl<'de> Deserialize<'de> for State {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        read_label(d)
    }
}

impl Serialize for Readiness {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let (state, reason) = match *self {
            Self::Selected => (State::Selected, None),
            Self::Uploading => (State::Uploading, None),
            Self::Ready => (State::Ready, None),
            Self::Failed { reason } => (State::Failed, Some(reason)),
            Self::Removed => (State::Removed, None),
            Self::Expired => (State::Expired, None),
        };
        let mut map = s.serialize_map(Some(1 + usize::from(reason.is_some())))?;
        map.serialize_entry("state", state.label())?;
        if let Some(reason) = reason {
            map.serialize_entry("reason", reason.label())?;
        }
        map.end()
    }
}

struct ReadinessVisitor;

impl<'de> Visitor<'de> for ReadinessVisitor {
    type Value = Readiness;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a readiness object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Readiness, A::Error> {
        let (mut state, mut reason): (Option<State>, Option<FailReason>) = (None, None);
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "state" => once(&mut map, &mut state, "state")?,
                "reason" => once(&mut map, &mut reason, "reason")?,
                other => return Err(de::Error::unknown_field(other, &["state", "reason"])),
            }
        }
        let state = state.ok_or_else(|| de::Error::missing_field("state"))?;
        Ok(match (state, reason) {
            (State::Failed, Some(reason)) => Readiness::Failed { reason },
            (State::Failed, None) => return Err(de::Error::missing_field("reason")),
            (_, Some(_)) => return Err(de::Error::unknown_field("reason", &["state"])),
            (State::Selected, None) => Readiness::Selected,
            (State::Uploading, None) => Readiness::Uploading,
            (State::Ready, None) => Readiness::Ready,
            (State::Removed, None) => Readiness::Removed,
            (State::Expired, None) => Readiness::Expired,
        })
    }
}

impl<'de> Deserialize<'de> for Readiness {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_map(ReadinessVisitor)
    }
}

impl Readiness {
    /// Whether this state may follow `self`. Nothing becomes ready except
    /// from an upload, a failure is left only by a new upload or removal,
    /// and `removed` and `expired` are final.
    pub fn can_become(self, next: Readiness) -> bool {
        use Readiness::*;
        matches!(
            (self, next),
            (Selected, Uploading | Removed)
                | (Uploading, Ready | Removed)
                | (Ready, Removed | Expired)
                | (Selected | Uploading | Ready, Failed { .. })
                | (Failed { .. }, Uploading | Removed)
        )
    }

    /// Only `ready` claims a verified copy on the controller.
    pub fn is_ready(self) -> bool {
        self == Readiness::Ready
    }
}

/// Where the operation that claimed a ready attachment stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Claim {
    /// No send or new thread uses it yet.
    Unclaimed,
    /// Its operation is being dispatched.
    Dispatching,
    /// Its operation's accepted outcome is recorded.
    Accepted,
    /// Its operation failed.
    Failed,
    /// Its operation's outcome is uncertain, until resolved.
    Uncertain,
}

/// What the store holds for one attachment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stored {
    /// An unfinished copy, left by a crash during staging.
    Partial,
    /// A ready copy, `age` after it became ready.
    Ready { claim: Claim, age: Duration },
}

/// Keep a stored attachment, or delete it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Retention {
    Keep,
    Delete(DeleteReason),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeleteReason {
    /// An unfinished copy.
    Partial,
    /// Unclaimed, or its operation failed, past [`UNSENT_TTL`].
    Expired,
    /// Its operation's accepted outcome is recorded; CloudRoom reads its
    /// own copy (F3, F8).
    Accepted,
}

/// The retention rule: a partial copy goes; a ready one goes
/// once its operation is accepted, or at [`UNSENT_TTL`] if unclaimed or
/// failed; it is kept while dispatching or uncertain.
pub fn retention(stored: Stored) -> Retention {
    match stored {
        Stored::Partial => Retention::Delete(DeleteReason::Partial),
        Stored::Ready { claim, age } => match claim {
            Claim::Accepted => Retention::Delete(DeleteReason::Accepted),
            Claim::Dispatching | Claim::Uncertain => Retention::Keep,
            Claim::Unclaimed | Claim::Failed if age >= UNSENT_TTL => {
                Retention::Delete(DeleteReason::Expired)
            }
            Claim::Unclaimed | Claim::Failed => Retention::Keep,
        },
    }
}

/// The owner can't remove an attachment its operation may still use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RemoveRefused(pub Claim);

/// Whether the owner may remove an attachment in this claim: not while its
/// operation is dispatching or uncertain.
pub fn can_remove(claim: Claim) -> Result<(), RemoveRefused> {
    match claim {
        Claim::Dispatching | Claim::Uncertain => Err(RemoveRefused(claim)),
        Claim::Unclaimed | Claim::Accepted | Claim::Failed => Ok(()),
    }
}

/// The directory of the state directory that holds the staged copies (the
/// store's `attachments/`). A [`StagedPath`] always ends in
/// `<STAGING_DIR>/<id>/<safe name>`.
pub const STAGING_DIR: &str = "attachments";

/// The absolute path of one staged copy, the only path `--file` and
/// `--image` may carry: `<area>/<id>/<safe name>`, where `area`
/// is the store's [`STAGING_DIR`], absolute, with no `.` or `..` component,
/// in UTF-8 and with no control character. The id and the safe name are
/// checked types, so neither can add a component or start an option.
///
/// It can be built only by [`StagedPath::new`], from those parts: no text,
/// relative token, `file:` URL or path from a message can become one. It is
/// never written to a report or a record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagedPath(String);

impl StagedPath {
    /// The staged copy of `id`, named `name`, in `area` (the store's
    /// `attachments/` directory).
    pub fn new(
        area: &std::path::Path,
        id: &AttachmentId,
        name: &SafeName,
    ) -> Result<Self, InvalidAttachment> {
        use std::path::Component;
        let refuse = || {
            Err(InvalidAttachment(format!(
                "a staged path is an absolute {STAGING_DIR} directory, with no . or .. \
                 component, in UTF-8 with no control character"
            )))
        };
        let Some(text) = area.to_str() else {
            return refuse();
        };
        // The text itself, segment by segment: `Path::components` would
        // drop a `.` in the middle and an empty segment, and the path is
        // passed as this text.
        let plain = text.strip_prefix('/').is_some_and(|rest| {
            rest.split('/')
                .all(|segment| !matches!(segment, "" | "." | ".."))
        }) && area
            .components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_)));
        let named = area.file_name().is_some_and(|n| n == STAGING_DIR);
        if !plain || !named || text.chars().any(char::is_control) {
            return refuse();
        }
        Ok(Self(format!("{text}/{}/{}", id.as_str(), name.as_str())))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One attachment of a prompt, as the op id binds it and a record keeps it:
/// its id, SHA-256, size, kind and staged name. Never its bytes, and never
/// the owner's original file name.
///
/// Read only from a map with exactly these five keys, each once (B1's
/// rule, not serde's derived reader).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PromptAttachment {
    pub id: AttachmentId,
    pub sha256: Sha256,
    pub size_bytes: u64,
    pub kind: Kind,
    pub name: SafeName,
}

impl From<&AttachmentRef> for PromptAttachment {
    fn from(a: &AttachmentRef) -> Self {
        Self {
            id: a.id.clone(),
            sha256: a.sha256.clone(),
            size_bytes: a.size_bytes,
            kind: a.kind,
            name: a.name.clone(),
        }
    }
}

/// The keys of a [`PromptAttachment`], in the order it is written.
pub const PROMPT_ATTACHMENT_KEYS: &[&str] = &["id", "sha256", "size_bytes", "kind", "name"];

struct PromptAttachmentVisitor;

impl<'de> Visitor<'de> for PromptAttachmentVisitor {
    type Value = PromptAttachment;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a prompt attachment object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<PromptAttachment, A::Error> {
        let (mut id, mut sha256, mut size_bytes, mut kind, mut name) =
            (None, None, None, None, None);
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "id" => once(&mut map, &mut id, "id")?,
                "sha256" => once(&mut map, &mut sha256, "sha256")?,
                "size_bytes" => once(&mut map, &mut size_bytes, "size_bytes")?,
                "kind" => once(&mut map, &mut kind, "kind")?,
                "name" => once(&mut map, &mut name, "name")?,
                other => return Err(de::Error::unknown_field(other, PROMPT_ATTACHMENT_KEYS)),
            }
        }
        Ok(PromptAttachment {
            id: checked(id, "id", AttachmentId::parse)?,
            sha256: checked(sha256, "sha256", Sha256::parse)?,
            size_bytes: size_bytes.ok_or_else(|| de::Error::missing_field("size_bytes"))?,
            kind: kind.ok_or_else(|| de::Error::missing_field("kind"))?,
            name: checked(name, "name", SafeName::parse)?,
        })
    }
}

impl<'de> Deserialize<'de> for PromptAttachment {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_map(PromptAttachmentVisitor)
    }
}

/// One ready attachment and the path of its staged copy: what the adapter
/// passes with `--file` or `--image`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagedAttachment {
    pub attachment: PromptAttachment,
    pub path: StagedPath,
}
