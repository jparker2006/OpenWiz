use openwiz_core::attachment::{SAFE_NAME_MAX_BYTES, SafeName};

use crate::support::*;

/// The Cloud core's rule for an attachment name (F5): at most 200 bytes,
/// no `/`, `\`, `:` or control byte, and never `.` or `..`.
fn cloud_core_accepts(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && name != "."
        && name != ".."
        && !name
            .bytes()
            .any(|b| matches!(b, b'/' | b'\\' | b':') || b.is_ascii_control())
}

/// CloudRoom's `sanitizeFilename` (source-read): Node's `basename`, then
/// every run of other characters becomes one `-`; empty becomes
/// `attachment`.
fn cloudroom_sanitize(name: &str) -> String {
    let base = name.rsplit('/').next().unwrap_or_default();
    let mut out = String::new();
    let mut in_run = false;
    for c in base.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
            out.push(c);
            in_run = false;
        } else if !in_run {
            out.push('-');
            in_run = true;
        }
    }
    if out.is_empty() {
        "attachment".to_owned()
    } else {
        out
    }
}

/// CloudRoom's `buildStoredFilename` (source-read): the sanitized name's
/// stem, `-`, `Date.now()`, `-`, up to six base-36 characters, then Node's
/// `extname`. `Date.now()` has 13 digits from 2001 to 2286.
fn cloudroom_stored_name(name: &str, now_ms: u64, random: &str) -> String {
    let sanitized = cloudroom_sanitize(name);
    let extension = match sanitized.rfind('.') {
        Some(dot) if dot > 0 => &sanitized[dot..],
        _ => "",
    };
    let stem = &sanitized[..sanitized.len() - extension.len()];
    format!("{stem}-{now_ms}-{random}{extension}")
}

/// The staged name: only these bytes, never `.` or `..`, never empty, never
/// a leading `-` or `.`, and at most [`SAFE_NAME_MAX_BYTES`].
fn is_valid(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= SAFE_NAME_MAX_BYTES
        && name != "."
        && name != ".."
        && !name.starts_with('-')
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Hostile and awkward originals, all synthetic.
fn originals() -> Vec<String> {
    let mut names: Vec<String> = [
        "synthetic.png",
        "Synthetic Report (final).pdf",
        "résumé-synthétique.txt",
        "報告書.pdf",
        "😀😀.png",
        "../../etc/passwd",
        "..\\..\\windows\\synthetic.ini",
        "/abs/path/to/synthetic.txt",
        "C:\\Users\\synthetic\\notes.txt",
        "folder/",
        "folder\\",
        ".",
        "..",
        "...",
        "",
        " ",
        "-rf",
        "--help",
        "-",
        ".hidden",
        ".env",
        "synthetic\u{0}name.txt",
        "synthetic\nname.txt",
        "synthetic\r\n.txt",
        "synthetic\u{7f}.txt",
        "synthetic\u{202e}gnp.exe",
        "\u{202a}synthetic\u{2066}.txt\u{2069}",
        "\u{200b}\u{feff}",
        "synthetic:stream.txt",
        "a/b:c\\d.txt",
        "name.",
        "trailing.space .txt",
        "%2e%2e%2fsynthetic",
        "synthetic.tar.gz",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    names.push("a".repeat(500) + ".png");
    names.push("é".repeat(300) + ".jpg");
    names.push("b".repeat(250));
    names.push(".".repeat(300));
    names.push("x.".to_owned() + &"y".repeat(400));
    names
}

#[test]
fn every_original_gives_a_valid_safe_name() {
    for original in originals() {
        let safe = SafeName::from_original(&original);
        assert!(
            is_valid(safe.as_str()),
            "{original:?} -> {:?}",
            safe.as_str()
        );
        // It is also what the core accepts back, from a record on disk.
        assert_eq!(
            SafeName::parse(safe.as_str()),
            Ok(safe.clone()),
            "{original:?}"
        );
    }
}

#[test]
fn the_safe_name_is_a_fixed_point_of_cloudrooms_reduction() {
    // CloudRoom's sanitizing changes nothing in a staged name, so the
    // stored name is the staged stem and extension, plus CloudRoom's
    // suffix, and nothing else.
    for original in originals() {
        let safe = SafeName::from_original(&original);
        assert_eq!(
            cloudroom_sanitize(safe.as_str()),
            safe.as_str(),
            "{original:?}"
        );
        assert_eq!(SafeName::from_original(safe.as_str()), safe, "idempotent");
    }
}

#[test]
fn cloudrooms_stored_name_fits_the_cloud_cores_limit() {
    let longest_suffix = "-9999999999999-zzzzzz".len();
    assert_eq!(longest_suffix, 21);
    assert_eq!(SAFE_NAME_MAX_BYTES, 200 - longest_suffix, "179");
    for original in originals() {
        let safe = SafeName::from_original(&original);
        for (now_ms, random) in [
            (9_999_999_999_999, "zzzzzz"),
            (1_793_000_000_000, "a1b2c3"),
            (1_000_000_000_000, "z"),
        ] {
            let stored = cloudroom_stored_name(safe.as_str(), now_ms, random);
            assert!(
                cloud_core_accepts(&stored),
                "{original:?} -> {:?} -> {stored:?} ({} bytes)",
                safe.as_str(),
                stored.len()
            );
            // The suffix goes before the extension, so CloudRoom types the
            // stored copy as it types the staged name.
            let stored_extension = stored.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
            if safe.extension().is_some() {
                assert_eq!(stored_extension, safe.extension(), "{original:?}");
            }
            assert!(stored.starts_with(safe.as_str().split('.').next().unwrap()));
        }
    }
    // The staged name at its limit, with an extension: exactly 200 stored.
    let at_limit = SafeName::from_original(&("a".repeat(500) + ".png"));
    assert_eq!(at_limit.as_str().len(), SAFE_NAME_MAX_BYTES);
    assert_eq!(
        cloudroom_stored_name(at_limit.as_str(), 9_999_999_999_999, "zzzzzz").len(),
        200
    );
}

#[test]
fn only_the_basename_is_kept() {
    for (original, safe) in [
        ("synthetic.png", "synthetic.png"),
        ("Synthetic-Notes_v2.txt", "Synthetic-Notes_v2.txt"),
        ("../../etc/passwd", "passwd"),
        ("..\\..\\windows\\synthetic.ini", "synthetic.ini"),
        ("/abs/path/to/synthetic.txt", "synthetic.txt"),
        ("C:\\Users\\synthetic\\notes.txt", "notes.txt"),
        (
            "Synthetic Report (final).pdf",
            "Synthetic_Report__final_.pdf",
        ),
        ("synthetic:stream.txt", "synthetic_stream.txt"),
        ("synthetic\u{202e}gnp.exe", "synthetic_gnp.exe"),
        ("résumé.txt", "r_sum_.txt"),
        ("-rf", "_rf"),
        ("--help", "__help"),
        (".env", "_env"),
        ("..", "attachment"),
        (".", "attachment"),
        ("", "attachment"),
        ("folder/", "attachment"),
        ("synthetic.tar.gz", "synthetic.tar.gz"),
    ] {
        assert_eq!(
            SafeName::from_original(original).as_str(),
            safe,
            "{original:?}"
        );
    }
}

#[test]
fn a_long_name_keeps_its_extension() {
    let safe = SafeName::from_original(&("a".repeat(500) + ".png"));
    assert_eq!(safe.as_str().len(), SAFE_NAME_MAX_BYTES);
    assert!(safe.as_str().ends_with(".png"));
    assert_eq!(safe.extension().as_deref(), Some("png"));
    // Multi-byte characters become one `_` each, then the same cut.
    let safe = SafeName::from_original(&("é".repeat(300) + ".jpg"));
    assert_eq!(safe.as_str().len(), SAFE_NAME_MAX_BYTES);
    assert!(safe.as_str().ends_with(".jpg"));
    // An extension too long to keep is cut with the rest.
    let safe = SafeName::from_original(&("x.".to_owned() + &"y".repeat(400)));
    assert_eq!(safe.as_str().len(), SAFE_NAME_MAX_BYTES);
}

#[test]
fn the_extension_is_the_safe_names_last_suffix_in_lowercase() {
    for (original, extension) in [
        ("synthetic.PNG", Some("png")),
        ("synthetic.tar.gz", Some("gz")),
        ("synthetic", None),
        ("name.", None),
        (".env", None),
        ("photo.jpg\u{202e}", Some("jpg_")),
    ] {
        assert_eq!(
            SafeName::from_original(original).extension().as_deref(),
            extension,
            "{original:?}"
        );
    }
}

#[test]
fn an_invalid_name_from_a_record_is_refused() {
    for bad in [
        "", ".", "..", "-rf", ".env", "a/b", "a\\b", "a b", "é.txt", "a\u{0}b",
    ] {
        assert!(SafeName::parse(bad).is_err(), "{bad:?}");
    }
    assert!(SafeName::parse(&"a".repeat(SAFE_NAME_MAX_BYTES + 1)).is_err());
    assert!(SafeName::parse(&"a".repeat(SAFE_NAME_MAX_BYTES)).is_ok());
    let invalid: Result<SafeName, _> = serde_json::from_value(serde_json::json!("../x"));
    assert!(invalid.is_err(), "deserializing goes through the check");
}

#[test]
fn the_display_name_keeps_the_original_as_text() {
    use openwiz_core::attachment::{AttachmentId, AttachmentRef, Kind, Sha256};
    for original in originals() {
        let reference = AttachmentRef {
            id: AttachmentId::parse("att_syn_0001").unwrap(),
            sha256: Sha256::parse(&"0".repeat(64)).unwrap(),
            size_bytes: 1,
            kind: Kind::File,
            name: SafeName::from_original(&original),
            display_name: original.clone(),
        };
        let json = serde_json::to_value(&reference).unwrap();
        assert_eq!(
            json["display_name"],
            original.as_str(),
            "unchanged, as a string"
        );
    }
    let _ = name("unused");
}
