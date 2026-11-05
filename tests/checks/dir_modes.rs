use std::fs;
use std::path::{Path, PathBuf};

const TESTS: &str = env!("CARGO_MANIFEST_DIR");

/// The shell command that makes a directory.
const SHELL_MAKE_DIR: &str = concat!("mk", "dir");

/// Every file under `dir`, sorted.
fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            files(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Where the comment or literal that starts at `i` ends (exclusive), if one
/// does: a line or block comment (nested), a string or byte string (with
/// escapes), a raw string (`r"…"`, `br#"…"#`), or a character or byte
/// literal. A `'` that starts none (a lifetime or a label) is code.
fn skipped_from(chars: &[char], i: usize) -> Option<usize> {
    let at = |j: usize| chars.get(j).copied();
    let find = |from: usize, pred: &dyn Fn(usize) -> bool| {
        (from..chars.len())
            .find(|&j| pred(j))
            .unwrap_or(chars.len())
    };
    match (chars[i], at(i + 1)) {
        ('/', Some('/')) => Some(find(i, &|j| chars[j] == '\n')),
        ('/', Some('*')) => {
            let mut depth = 0;
            let mut j = i;
            while j < chars.len() {
                match (chars[j], at(j + 1)) {
                    ('/', Some('*')) => {
                        depth += 1;
                        j += 2;
                    }
                    ('*', Some('/')) => {
                        depth -= 1;
                        j += 2;
                        if depth == 0 {
                            return Some(j);
                        }
                    }
                    _ => j += 1,
                }
            }
            Some(chars.len())
        }
        ('"', _) => {
            let mut j = i + 1;
            while j < chars.len() {
                match chars[j] {
                    '\\' => j += 2,
                    '"' => return Some(j + 1),
                    _ => j += 1,
                }
            }
            Some(chars.len())
        }
        ('r', _) => {
            let start = (0..=i).rev().take_while(|&j| ident(chars[j])).last()?;
            let prefix: String = chars[start..=i].iter().collect();
            let hashes = (i + 1..chars.len())
                .take_while(|&j| chars[j] == '#')
                .count();
            if !matches!(prefix.as_str(), "r" | "br" | "cr") || at(i + 1 + hashes) != Some('"') {
                return None;
            }
            let body = i + 2 + hashes;
            let end = find(body, &|j| {
                chars[j] == '"' && (1..=hashes).all(|k| at(j + k) == Some('#'))
            });
            Some((end + 1 + hashes).min(chars.len()))
        }
        ('\'', Some('\\')) => Some(find(i + 3, &|j| chars[j] == '\'') + 1),
        ('\'', Some(_)) if at(i + 2) == Some('\'') => Some(i + 3),
        _ => None,
    }
}

/// `source` with every comment and literal blanked to spaces, newlines
/// kept so line numbers hold: what is left is code.
fn code_only(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut code = String::with_capacity(source.len());
    let mut i = 0;
    while i < chars.len() {
        if let Some(end) = skipped_from(&chars, i) {
            let end = end.min(chars.len());
            code.extend(
                chars[i..end]
                    .iter()
                    .map(|&c| if c == '\n' { c } else { ' ' }),
            );
            i = end;
        } else {
            code.push(chars[i]);
            i += 1;
        }
    }
    code
}

/// The byte offsets of `word` in `text`, as a whole word.
fn whole_words(text: &str, word: &str) -> Vec<usize> {
    text.match_indices(word)
        .map(|(at, _)| at)
        .filter(|&at| {
            let before = text[..at].chars().next_back();
            let after = text[at + word.len()..].chars().next();
            !before.is_some_and(ident) && !after.is_some_and(ident)
        })
        .collect()
}

/// The names of the methods called in the chain that starts at `code`
/// (`.a(..).b(..)`), in order, up to where the chain ends. A call inside an
/// argument is not the chain's. `code` has its comments and literals blanked,
/// so every parenthesis in it is code.
fn chain_methods(code: &str) -> Vec<&str> {
    let mut methods = Vec::new();
    let mut rest = code;
    while let Some(call) = rest.trim_start().strip_prefix('.') {
        let call = call.trim_start();
        let (name, args) = call.split_at(call.find(|c: char| !ident(c)).unwrap_or(call.len()));
        let Some(args) = args.trim_start().strip_prefix('(') else {
            break;
        };
        let mut depth = 1;
        let close = args.char_indices().find(|&(_, c)| {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            depth == 0
        });
        let Some((close, _)) = close else {
            break;
        };
        methods.push(name);
        rest = &args[close + 1..];
    }
    methods
}

fn line_of(text: &str, at: usize) -> usize {
    text[..at].matches('\n').count() + 1
}

/// The findings for one file's text, each `<name>:<line>: <what>`, in line
/// order.
fn findings(name: &str, source: &str) -> Vec<String> {
    let mut found: Vec<(usize, String)> = Vec::new();
    for (at, _) in source.match_indices(SHELL_MAKE_DIR) {
        let what = "a shell command makes a directory with the umask's mode".to_owned();
        found.push((line_of(source, at), what));
    }
    if name.ends_with(".rs") {
        let code = code_only(source);
        for word in ["create_dir", "create_dir_all"] {
            for at in whole_words(&code, word) {
                let what =
                    format!("{word} leaves the mode to the umask; use tests/support/dirs.rs");
                found.push((line_of(&code, at), what));
            }
        }
        let builders = whole_words(&code, "DirBuilder");
        for &at in &builders {
            let rest = code[at + "DirBuilder".len()..].trim_start();
            if rest.starts_with([',', ';', '}']) {
                continue; // a `use` list
            }
            let line = line_of(&code, at);
            let Some(chain) = rest.strip_prefix("::new()") else {
                let what = "DirBuilder only as DirBuilder::new() or in a use list";
                found.push((line, what.to_owned()));
                continue;
            };
            let methods = chain_methods(chain);
            let made = methods.iter().position(|&m| m == "create");
            if !made.is_some_and(|at| methods[..at].contains(&"mode")) {
                let what = "this DirBuilder sets no .mode(..) before its .create(..)";
                found.push((line, what.to_owned()));
            }
        }
    }
    found.sort();
    found
        .into_iter()
        .map(|(line, what)| format!("{name}:{line}: {what}"))
        .collect()
}

/// Every finding under `tests/`, and every file read.
fn scan() -> (Vec<String>, Vec<String>) {
    let root = Path::new(TESTS);
    let mut paths = Vec::new();
    files(root, &mut paths);
    paths.sort();
    let mut found = Vec::new();
    let mut names = Vec::new();
    for path in paths {
        let name = path
            .strip_prefix(root)
            .expect("under tests/")
            .to_string_lossy()
            .into_owned();
        let source = String::from_utf8_lossy(&fs::read(&path).expect("read")).into_owned();
        found.extend(findings(&name, &source));
        names.push(name);
    }
    (found, names)
}

#[test]
fn no_test_leaves_a_directorys_mode_to_the_umask() {
    let (found, names) = scan();
    assert!(
        found.is_empty(),
        "{} directories a test makes with the umask's mode:\n{}",
        found.len(),
        found.join("\n")
    );
    // The walk reached nested folders and every kind of file.
    for name in [
        "checks/dir_modes.rs",
        "messaging/ad_send_attachments.rs",
        "fixtures/README.md",
        "Cargo.toml",
    ] {
        assert!(names.iter().any(|n| n == name), "{name} was not read");
    }
}

#[test]
fn the_check_catches_every_planted_form() {
    let shell = format!("fs::write(&script, \"#!/bin/sh\\n{SHELL_MAKE_DIR} -p state\\n\")");
    let comment = format!("// {SHELL_MAKE_DIR} the state directory first");
    // (what, source, findings)
    let cases: &[(&str, &str, usize)] = &[
        // Refused.
        ("call", "fs::create_dir_all(&state).unwrap();", 1),
        ("qualified call", "std::fs::create_dir(&state).unwrap();", 1),
        (
            "alias",
            "use std::fs::create_dir_all as make;\nmake(&state).unwrap();",
            1,
        ),
        (
            "function value",
            "dirs.iter().try_for_each(fs::create_dir_all).unwrap();",
            1,
        ),
        (
            "builder without a mode",
            "fs::DirBuilder::new().recursive(true).create(&state).unwrap();",
            1,
        ),
        (
            "formatted builder without a mode",
            "fs::DirBuilder::new()\n    .recursive(true)\n    .create(&state)\n    .unwrap();",
            1,
        ),
        (
            "stored builder without a mode",
            "let mut b = DirBuilder::new();\nb.recursive(true);\nb.create(&state).unwrap();",
            1,
        ),
        (
            "another builder's mode",
            "let a = DirBuilder::new();\nlet b = DirBuilder::new().mode(0o700);\n\
             a.create(&s).unwrap();\nb.create(&t).unwrap();",
            2,
        ),
        (
            "an unrelated mode before a stored builder's create",
            "let b = DirBuilder::new();\nlet mut f = OpenOptions::new();\nf.mode(0o600);\n\
             b.create(&state).unwrap();",
            1,
        ),
        (
            "stored builder with its own mode",
            "let mut b = DirBuilder::new();\nb.mode(0o700);\nb.create(&state).unwrap();",
            1,
        ),
        (
            "a mode inside an argument",
            "DirBuilder::new().recursive(opts(OpenOptions::new().mode(0o600))).create(&state).unwrap();",
            1,
        ),
        (
            "a mode after its create",
            "DirBuilder::new().create(&state).unwrap().mode(0o700);",
            1,
        ),
        (
            "builder alias",
            "use std::fs::DirBuilder as Make;\nMake::new().create(&state).unwrap();",
            1,
        ),
        (
            "builder that never creates",
            "let b = DirBuilder::new().mode(0o700);",
            1,
        ),
        ("shell script", &shell, 1),
        ("comment", &comment, 1),
        (
            "code after a lifetime and a char literal",
            "fn f<'a>(p: &'a Path) -> char { let q = '\"'; fs::create_dir(p); q }",
            1,
        ),
        (
            "code after an escaped char literal",
            "let q = b'\\''; fs::create_dir_all(p).unwrap();",
            1,
        ),
        // Accepted.
        (
            "builder with a mode",
            "fs::DirBuilder::new().recursive(true).mode(0o700).create(&state).unwrap();",
            0,
        ),
        (
            "formatted builder with a mode",
            "fs::DirBuilder::new()\n    .recursive(true)\n    .mode(0o755)\n    .create(&state)\n    .unwrap();",
            0,
        ),
        (
            "use lists",
            "use std::fs::{self, DirBuilder};\nuse std::fs::DirBuilder;\nuse std::os::unix::fs::DirBuilderExt;",
            0,
        ),
        (
            "the helper",
            "crate::dirs::private_dir_all(&state).unwrap();",
            0,
        ),
        (
            "in a string",
            "assert!(body.contains(\"create_dir\"), \"{body}\");",
            0,
        ),
        (
            "an escaped quote in a string",
            "let s = \"a \\\" fs::create_dir_all(p) \\\"\";",
            0,
        ),
        (
            "in a raw string",
            "let s = r#\"fs::create_dir_all(\"p\")\"#;",
            0,
        ),
        (
            "in a comment",
            "// fs::create_dir_all would use the umask",
            0,
        ),
        ("in a doc comment", "/// `create_dir` fails if it exists", 0),
        (
            "in a nested block comment",
            "/* create_dir /* nested */ create_dir_all */ let x = 1;",
            0,
        ),
        ("a longer name", "fn create_dir_synced() {}", 0),
        (
            "a lifetime before a string",
            "fn f(_: &'static str) -> &'static str { \"create_dir(\" }",
            0,
        ),
    ];
    let wrong: Vec<String> = cases
        .iter()
        .filter_map(|(what, source, want)| {
            let found = findings("planted.rs", source);
            (found.len() != *want).then(|| format!("{what}: want {want}, got {found:?}"))
        })
        .collect();
    assert_eq!(
        wrong,
        Vec::<String>::new(),
        "planted cases the check got wrong"
    );
    // Each finding names its file and line.
    assert_eq!(
        findings(
            "planted.rs",
            "let a = 1;\n\nfs::create_dir_all(&state).unwrap();"
        ),
        ["planted.rs:3: create_dir_all leaves the mode to the umask; use tests/support/dirs.rs"]
    );
    // The shell rule reads every kind of file; the Rust rules only `.rs`.
    assert_eq!(findings("planted.json", &shell).len(), 1);
    assert!(findings("planted.json", "fs::create_dir_all(&state)").is_empty());
}
