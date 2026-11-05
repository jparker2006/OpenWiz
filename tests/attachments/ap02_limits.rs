//! AP2 Provider limits only. 10 MiB per image and 25 MiB per file, except a
//! file whose extension CloudRoom types as `image/*`, which gets the image
//! limit (F26). No count cap, and no other size or count constant.

use std::fs;
use std::path::Path;

use openwiz_core::attachment::{
    FILE_MAX_BYTES, FailReason, IMAGE_MAX_BYTES, IMAGE_TYPED_EXTENSIONS, Kind, check, limit,
};

use crate::support::*;

#[test]
fn the_limits_are_cloudrooms() {
    assert_eq!(IMAGE_MAX_BYTES, 10 * MIB, "F7: 10 MiB per image");
    assert_eq!(FILE_MAX_BYTES, 25 * MIB, "F7: 25 MiB per file");
    assert_eq!(
        FailReason::ImageTooLarge.message(),
        "CloudRoom accepts images up to 10 MiB"
    );
    assert_eq!(
        FailReason::FileTooLarge.message(),
        "CloudRoom accepts files up to 25 MiB"
    );
}

#[test]
fn an_image_at_10_mib_is_accepted_and_one_byte_over_is_refused() {
    for (file, magic) in [
        ("synthetic.png", PNG),
        ("synthetic.jpg", JPEG),
        ("synthetic.gif", GIF89),
        ("synthetic.webp", WEBP),
    ] {
        let at = padded(magic, IMAGE_MAX_BYTES);
        let over = padded(magic, IMAGE_MAX_BYTES + 1);
        assert_eq!(at.len() as u64, 10 * MIB);
        let n = name(file);
        assert_eq!(check(&n, &at, at.len() as u64), Ok(Kind::Image), "{file}");
        assert_eq!(
            check(&n, &over, over.len() as u64),
            Err(FailReason::ImageTooLarge),
            "{file}"
        );
    }
}

#[test]
fn a_file_at_25_mib_is_accepted_and_one_byte_over_is_refused() {
    for (file, magic) in [
        ("synthetic.pdf", PDF),
        ("synthetic.zip", ZIP),
        ("synthetic.txt", TEXT),
        ("synthetic", TEXT),
    ] {
        let at = padded(magic, FILE_MAX_BYTES);
        let over = padded(magic, FILE_MAX_BYTES + 1);
        assert_eq!(at.len() as u64, 25 * MIB);
        let n = name(file);
        assert_eq!(check(&n, &at, at.len() as u64), Ok(Kind::File), "{file}");
        assert_eq!(
            check(&n, &over, over.len() as u64),
            Err(FailReason::FileTooLarge),
            "{file}"
        );
    }
}

#[test]
fn a_file_cloudroom_types_as_an_image_gets_the_image_limit() {
    for (file, magic) in [
        ("synthetic.svg", SVG),
        ("synthetic.bmp", BMP),
        ("synthetic.tiff", TIFF),
        ("synthetic.TIF", TIFF),
        ("synthetic.avif", &b"\0\0\0\x1cftypavif"[..]),
        ("synthetic.ico", TEXT),
    ] {
        let n = name(file);
        assert_eq!(limit(Kind::File, &n).max_bytes, IMAGE_MAX_BYTES, "{file}");
        let at = padded(magic, IMAGE_MAX_BYTES);
        let over = padded(magic, IMAGE_MAX_BYTES + 1);
        assert_eq!(check(&n, &at, at.len() as u64), Ok(Kind::File), "{file}");
        assert_eq!(
            check(&n, &over, over.len() as u64),
            Err(FailReason::ImageTypedFileTooLarge),
            "{file}"
        );
    }
    assert!(
        FailReason::ImageTypedFileTooLarge
            .message()
            .contains("10 MiB")
    );
    // Not in the table: the file limit.
    for file in [
        "synthetic.pdf",
        "synthetic.txt",
        "synthetic.zip",
        "synthetic",
    ] {
        assert_eq!(limit(Kind::File, &name(file)).max_bytes, FILE_MAX_BYTES);
    }
}

/// The table is CloudRoom's typing (F26), pinned: the extensions that
/// `mime-types` 2.1.35 (`mime-db` 1.52.0) or 3.0.2 (`mime-db` 1.54.0) looks
/// up as `image/*`, as recorded in the release checks.
#[test]
fn the_extension_table_is_pinned() {
    const PINNED: &str = "3ds apng avci avcs avif azv b16 bmp btf btif cgm cmx dds dib djv \
        djvu dng dpx drle dwg dxf emf exr fbs fh fh4 fh5 fh7 fhc fits fpx fst g3 gif heic heics \
        heif heifs hej2 hsj2 ico ief jaii jais jfif jhc jls jng jp2 jpe jpeg jpf jpg jpg2 jpgm \
        jph jpm jpx jxl jxr jxra jxrs jxs jxsc jxsi jxss ktx ktx2 mdi mmr npx pbm pct pcx pgm \
        pic png pnm ppm psd pti ras rgb rlc sgi sid svg svgz t38 tap tfx tga tif tiff uvg uvi \
        uvvg uvvi vtf wbmp wdp webp wmf xbm xif xpm xwd";
    let pinned: Vec<&str> = PINNED.split_whitespace().collect();
    assert_eq!(pinned.len(), 107);
    assert_eq!(IMAGE_TYPED_EXTENSIONS, pinned.as_slice());
    let mut sorted = pinned.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted, pinned, "sorted, unique and lowercase");
    let recorded = fs::read_to_string(Path::new(ROOT).join("docs/attachment-types.md"))
        .expect("the table's source is recorded");
    assert!(
        recorded.contains(
            PINNED
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .as_str()
        )
    );
}

#[test]
fn there_is_no_count_cap() {
    // 200 small files and 200 small images: every one is accepted on its
    // own, and nothing in the model counts them.
    let accepted = (0..200)
        .map(|i| {
            let file = name(&format!("synthetic-{i:03}.txt"));
            let image = name(&format!("synthetic-{i:03}.png"));
            check(&file, TEXT, TEXT.len() as u64) == Ok(Kind::File)
                && check(&image, PNG, 1024) == Ok(Kind::Image)
        })
        .filter(|ok| *ok)
        .count();
    assert_eq!(accepted, 200);
}

/// Every constant in the attachment model is one of these, and nothing else
/// in the core names an attachment cap. Adding a count or size cap fails
/// here (the negative check in the evidence).
#[test]
fn no_other_size_or_count_constant() {
    const ALLOWED: &[&str] = &[
        "ATTACHMENT_SCHEMA_ID",
        "IMAGE_MAX_BYTES",
        "FILE_MAX_BYTES",
        "IMAGE_TYPED_EXTENSIONS",
        "CLOUDROOM_NAME_MAX_BYTES",
        "CLOUDROOM_SUFFIX_MAX_BYTES",
        "SAFE_NAME_MAX_BYTES",
        "ATTACHMENT_ID_MAX_LEN",
        "SNIFF_BYTES",
        "UNSENT_TTL",
        "FREE_DISK_FLOOR_BYTES",
        // B2 (the implementation): a directory's name and a reader's keys, not
        // limits.
        "STAGING_DIR",
        "PROMPT_ATTACHMENT_KEYS",
    ];
    let model = fs::read_to_string(Path::new(ROOT).join("crates/core/src/attachment.rs")).unwrap();
    let mut found = Vec::new();
    for line in model.lines() {
        let line = line.trim_start();
        let line = line.strip_prefix("pub ").unwrap_or(line);
        for keyword in ["const ", "static "] {
            if let Some(rest) = line.strip_prefix(keyword) {
                let ident: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if !ident.is_empty() && ident != "_" {
                    found.push(ident);
                }
            }
        }
    }
    found.sort();
    let mut allowed: Vec<String> = ALLOWED.iter().map(|s| (*s).to_owned()).collect();
    allowed.sort();
    assert_eq!(found, allowed);
    for word in [
        "MAX_ATTACHMENTS",
        "ATTACHMENTS_MAX",
        "MAX_FILES",
        "TOTAL_BYTES",
    ] {
        assert!(!model.contains(word), "the model names {word}");
    }
    // The rest of the core doesn't define an attachment limit either.
    let mut files = Vec::new();
    rust_files(&Path::new(ROOT).join("crates/core/src"), &mut files);
    // B2 (the implementation) names three constants that are not limits: the two
    // headers of the attachments digest's encoding (`prompt.rs`) and the
    // words of the Steer refusal (`messaging/send.rs`).
    const NOT_LIMITS: &[&str] = &[
        "const EMPTY_HEADER:",
        "const ATTACHMENTS_HEADER:",
        "const STEER_WITH_ATTACHMENTS:",
    ];
    for path in files.iter().filter(|p| !p.ends_with("attachment.rs")) {
        let text = fs::read_to_string(path).unwrap();
        for line in text
            .lines()
            .filter(|l| l.contains("const ") && !NOT_LIMITS.iter().any(|n| l.contains(n)))
        {
            let upper = line.to_ascii_uppercase();
            assert!(
                !(upper.contains("ATTACH") || upper.contains("IMAGE") || upper.contains("UPLOAD")),
                "{path:?}: {line}"
            );
        }
    }
}

/// Phase 2: the store stages 200 small files and images in one `add`, each
/// under its own id. Nothing counts them.
#[test]
fn two_hundred_small_files_all_become_ready_in_the_store() {
    let bench = crate::bench::Bench::new("ap02_two_hundred");
    let paths: Vec<String> = (0..200)
        .map(|i| {
            let path = if i % 2 == 0 {
                bench.source(&format!("syn-ap02-{i:03}.txt"), TEXT)
            } else {
                bench.source(&format!("syn-ap02-{i:03}.png"), &padded(PNG, 64))
            };
            path.to_str().unwrap().to_owned()
        })
        .collect();
    let mut args = vec!["add", "--json"];
    args.extend(paths.iter().map(String::as_str));
    let run = bench.cli(&args);
    assert_eq!(run.code, 0, "{}", run.err);
    let json = run.json();
    assert_eq!(json["ready"].as_array().unwrap().len(), 200);
    assert!(json["failed"].as_array().unwrap().is_empty());
    assert_eq!(bench.ids().len(), 200);
}

/// Phase 2: the store's attachment module and the CLI's declare only these
/// constants: names, a buffer size, exit codes and schema ids. No count or
/// size cap.
#[test]
fn the_store_and_the_cli_add_no_cap() {
    for (file, allowed) in [
        (
            "crates/store/src/attachments.rs",
            &[
                "COPY_CHUNK_BYTES",
                // `Area`'s: the directory's name and what it is called.
                "DIR",
                "WHAT",
                "META",
                "PARTIAL_SUFFIX",
                "STAGING_LOCK",
                "ID_ATTEMPTS",
                // An open flag's value, not a limit.
                "O_NONBLOCK",
                // The free-space measure: its program, arguments and header.
                "DF_ARGS",
                "DF_HEADER",
                "DF_PROGRAM",
                // B2 (the implementation): the claims lock and the claim file.
                "CLAIMS_LOCK",
                "CLAIM",
                "ADD_SCHEMA_ID",
                "LIST_SCHEMA_ID",
                "REMOVE_SCHEMA_ID",
            ][..],
        ),
        (
            "crates/cli/src/attachment.rs",
            &[
                "EXIT_ATTACHMENT_FAILED",
                "EXIT_ATTACHMENT_NOT_FOUND",
                "EXIT_ATTACHMENT_REFUSED",
                "EXIT_ATTACHMENT_STORE_UNAVAILABLE",
            ][..],
        ),
    ] {
        let text = fs::read_to_string(Path::new(ROOT).join(file)).unwrap();
        let mut found: Vec<String> = Vec::new();
        for line in text.lines() {
            let line = line.trim_start();
            let line = line
                .strip_prefix("pub(crate) ")
                .or_else(|| line.strip_prefix("pub "))
                .unwrap_or(line);
            for keyword in ["const ", "static "] {
                if let Some(rest) = line.strip_prefix(keyword) {
                    let ident: String = rest
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                        .collect();
                    found.push(ident);
                }
            }
        }
        found.sort();
        let mut allowed: Vec<String> = allowed.iter().map(|s| (*s).to_owned()).collect();
        allowed.sort();
        assert_eq!(found, allowed, "{file}");
        for word in [
            "MAX_ATTACHMENTS",
            "ATTACHMENTS_MAX",
            "MAX_FILES",
            "TOTAL_BYTES",
            "MAX_BYTES",
        ] {
            assert!(!text.contains(word), "{file} names {word}");
        }
    }
}

fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}
