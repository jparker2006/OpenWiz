//! Shared helpers for the attachment proofs: synthetic file bytes, built
//! in memory at test time (never committed), and the attachment schema.

#![allow(dead_code)]

use std::fs;

use openwiz_core::attachment::SafeName;
use serde_json::{Value, json};

pub const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

pub const MIB: u64 = 1024 * 1024;

/// A synthetic file: `magic`, padded with `0x5a` to exactly `len` bytes.
pub fn padded(magic: &[u8], len: u64) -> Vec<u8> {
    let mut bytes = magic.to_vec();
    bytes.resize(usize::try_from(len).unwrap().max(magic.len()), 0x5a);
    bytes
}

pub const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR";
pub const JPEG: &[u8] = b"\xff\xd8\xff\xe0\0\x10JFIF\0";
pub const GIF87: &[u8] = b"GIF87a\x01\0\x01\0";
pub const GIF89: &[u8] = b"GIF89a\x01\0\x01\0";
pub const WEBP: &[u8] = b"RIFF\x24\0\0\0WEBPVP8 ";
pub const PDF: &[u8] = b"%PDF-1.7\n%synthetic\n";
pub const ZIP: &[u8] = b"PK\x03\x04\x14\0\0\0\x08\0";
pub const BMP: &[u8] = b"BM\x36\0\0\0\0\0\0\0\x36\0";
pub const TIFF: &[u8] = b"II*\0\x08\0\0\0";
pub const SVG: &[u8] =
    b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>synthetic()</script></svg>";
pub const HTML: &[u8] = b"<!doctype html><title>Synthetic</title>";
pub const TEXT: &[u8] = b"Synthetic text, reply ok.\n";

/// An ISO base media `ftyp` box: size, `ftyp`, the major brand, a minor
/// version and the compatible brands.
pub fn ftyp(major: &[u8; 4], compatible: &[&[u8; 4]]) -> Vec<u8> {
    let size = 16 + 4 * compatible.len();
    let mut bytes = u32::try_from(size).unwrap().to_be_bytes().to_vec();
    bytes.extend_from_slice(b"ftyp");
    bytes.extend_from_slice(major);
    bytes.extend_from_slice(&[0, 0, 0, 0]);
    for brand in compatible {
        bytes.extend_from_slice(*brand);
    }
    bytes.extend_from_slice(b"\0\0\0\x08meta");
    bytes
}

pub fn heic() -> Vec<u8> {
    ftyp(b"heic", &[b"mif1", b"heic"])
}

pub fn heif() -> Vec<u8> {
    ftyp(b"mif1", &[b"mif1", b"heic"])
}

pub fn avif() -> Vec<u8> {
    ftyp(b"avif", &[b"avif", b"mif1", b"miaf"])
}

pub fn name(original: &str) -> SafeName {
    SafeName::from_original(original)
}

// The schema.

pub const ATTACHMENT_SCHEMA: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../contracts/attachment.v1.schema.json"
);

pub fn attachment_schema() -> Value {
    serde_json::from_str(&fs::read_to_string(ATTACHMENT_SCHEMA).expect("schema")).expect("JSON")
}

/// Validates `value` against `$defs/<def>` of the attachment schema, its
/// patterns included.
pub fn validate_def(def: &str, value: &Value) -> Vec<String> {
    let mut schema = attachment_schema();
    schema["$ref"] = json!(format!("#/$defs/{def}"));
    crate::pattern::validate(&schema, value)
}
