//! A8: `openwiz-server` serves the web page, `apps/web`'s static export,
//! from one configured directory (`--web-root`), on the same origin as the
//! API. Claims W1 to W11, one section each below.
//!
//! The export trees here are synthetic and made by the tests, except W8 and
//! W9, which read `apps/web/out` as `npm run build` makes it.

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use openwiz_server::{
    Config, MAX_FILE_BYTES, MAX_WEB_BYTES, MAX_WEB_FILES, PAGE_CSP, STATIC_CACHE, WEB_ROUTES,
    WebRoot,
};

use crate::fake::Fake;
use crate::http::{Response, TestServer, serve, serve_with, server_binary};

const WEB_OUT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../apps/web/out");
const WEB_SERVE_MJS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../apps/web/test/support/serve.mjs"
);

/// The fixed content-type table (W6).
const TYPES: &[(&str, &str)] = &[
    ("html", "text/html; charset=utf-8"),
    ("js", "text/javascript; charset=utf-8"),
    ("css", "text/css; charset=utf-8"),
    ("woff2", "font/woff2"),
    ("txt", "text/plain; charset=utf-8"),
];

const SECRET: &[u8] = b"SECRET-OUTSIDE-THE-WEB-ROOT";

/// A synthetic export tree, with traps inside and secrets beside it.
struct Tree {
    root: PathBuf,
    files: BTreeMap<&'static str, Vec<u8>>,
}

/// Every file the synthetic tree serves, by URL path, with its bytes.
fn served_files() -> BTreeMap<&'static str, Vec<u8>> {
    let font: Vec<u8> = (0..=255u8).cycle().take(3000).collect();
    BTreeMap::from([
        (
            "/",
            b"<!DOCTYPE html><html><head><title>synthetic</title></head></html>".to_vec(),
        ),
        (
            "/_next/static/chunks/main-0a1b2c3d.js",
            b"console.log(\"synthetic\");\n".to_vec(),
        ),
        (
            "/_next/static/chunks/pages/index-4e5f.js",
            b"void 0;\n".to_vec(),
        ),
        (
            "/_next/static/css/app-77aa.css",
            b"body{margin:0}\n".to_vec(),
        ),
        (
            "/_next/static/BUILDID_syn-1/_buildManifest.js",
            b"self.__BUILD_MANIFEST={};\n".to_vec(),
        ),
        ("/fonts/Face-var.woff2", font),
        ("/fonts/OFL.txt", b"synthetic licence text\n".to_vec()),
    ])
}

/// A clean export tree, with secrets beside it.
fn tree(name: &str) -> Tree {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join("a8").join(name);
    let _ = fs::remove_dir_all(&base);
    let root = base.join("out");
    let files = served_files();
    for (url, bytes) in &files {
        let rel = if *url == "/" { "index.html" } else { &url[1..] };
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
    }
    // Emitted by a build, never served.
    fs::write(root.join("404.html"), b"<!DOCTYPE html>404").unwrap();
    fs::write(root.join("robots.txt"), b"synthetic").unwrap();
    fs::create_dir_all(root.join("_next/BUILDID_syn-1")).unwrap();
    // Secrets beside the root.
    fs::write(base.join("secret.txt"), SECRET).unwrap();
    fs::write(base.join("secret.js"), SECRET).unwrap();
    fs::create_dir_all(base.join("outside/fonts")).unwrap();
    fs::write(base.join("outside/index.html"), SECRET).unwrap();
    fs::write(base.join("outside/fonts/OFL.txt"), SECRET).unwrap();
    fs::write(base.join("outside/fonts/secret.txt"), SECRET).unwrap();
    Tree { root, files }
}

fn mkfifo(path: &Path) {
    let made = Command::new("mkfifo").arg(path).status().expect("mkfifo");
    assert!(made.success(), "mkfifo");
}

fn symlink(target: &str, link: &Path) {
    std::os::unix::fs::symlink(target, link).expect("symlink");
}

fn web_config(root: &Path) -> Config {
    Config {
        web_root: Some(WebRoot::open(root).expect("a valid web root")),
        ..Config::default()
    }
}

fn serve_tree(fake: &Fake, tree: &Tree) -> TestServer {
    serve_with(fake, web_config(&tree.root))
}

fn assert_not_served(what: &str, r: &Response) {
    assert_eq!(r.status, 404, "{what}: {}", r.status_line);
    assert_eq!(r.error().0, "no_route", "{what}");
    assert_api_headers(what, r);
    assert!(
        !r.body.windows(SECRET.len()).any(|w| w == SECRET),
        "{what}: a secret outside the root was served"
    );
}

/// The API's own headers (a refusal on a web path is the API's error).
fn assert_api_headers(what: &str, r: &Response) {
    assert_eq!(r.header("Content-Type"), Some("application/json"), "{what}");
    assert_eq!(r.header("Cache-Control"), Some("no-store"), "{what}");
    assert_eq!(
        r.header("Content-Security-Policy"),
        Some("default-src 'none'; frame-ancestors 'none'"),
        "{what}"
    );
}

fn content_type(url: &str) -> &'static str {
    let file = if url == "/" { "index.html" } else { url };
    let ext = file.rsplit_once('.').expect("an extension").1;
    TYPES
        .iter()
        .find(|(e, _)| *e == ext)
        .map(|(_, t)| *t)
        .unwrap_or_else(|| panic!("{url}: no type"))
}

/// W7: exactly the page headers, once each; `bare` for a response to HEAD.
fn assert_page_headers(what: &str, url: &str, r: &Response, bare: bool) {
    let cache = if url == "/" { "no-store" } else { STATIC_CACHE };
    let mut want: Vec<(&str, String)> = vec![
        ("Content-Type", content_type(url).to_owned()),
        ("Cache-Control", cache.to_owned()),
        ("X-Content-Type-Options", "nosniff".to_owned()),
        ("Referrer-Policy", "no-referrer".to_owned()),
        ("X-Frame-Options", "DENY".to_owned()),
        ("Content-Security-Policy", PAGE_CSP.to_owned()),
        ("Connection", "close".to_owned()),
    ];
    if !bare {
        want.push(("Content-Length", r.body.len().to_string()));
    }
    let mut got: Vec<(String, String)> = r.headers.clone();
    got.sort();
    let mut want: Vec<(String, String)> =
        want.into_iter().map(|(n, v)| (n.to_owned(), v)).collect();
    want.sort();
    assert_eq!(got, want, "{what}: the page headers");
    assert!(r.status_line.starts_with("HTTP/1.1 200 "), "{what}");
}

// W1 ---------------------------------------------------------------------

#[test]
fn w1_without_a_web_root_web_paths_stay_404_and_head_405() {
    let fake = Fake::new("web_a8_w1", "normal");
    let s = serve(&fake);
    for url in served_files().keys() {
        let r = s.get(url);
        assert_eq!(r.status, 404, "{url}");
        assert_eq!(r.error().0, "no_route", "{url}");
        assert_api_headers(url, &r);
        let r = s.request("HEAD", url);
        assert_eq!(r.status, 405, "HEAD {url}");
        assert_eq!(r.header("Allow"), Some("GET"), "HEAD {url}");
    }
    assert!(fake.invocations().is_empty());
}

// W2 ---------------------------------------------------------------------

#[test]
fn w2_the_web_root_is_a_directory_with_a_regular_index() {
    let t = tree("w2");
    assert!(WebRoot::open(&t.root).is_ok());
    let base = t.root.parent().unwrap();
    // A web root reached through a symlink is the directory it names.
    symlink("out", &base.join("via-link"));
    assert!(WebRoot::open(&base.join("via-link")).is_ok());
    let no_index = base.join("no-index");
    fs::create_dir_all(&no_index).unwrap();
    let dir_index = base.join("dir-index");
    fs::create_dir_all(dir_index.join("index.html")).unwrap();
    let link_index = base.join("link-index");
    fs::create_dir_all(&link_index).unwrap();
    symlink("../out/index.html", &link_index.join("index.html"));
    for bad in [
        base.join("missing"),
        base.join("secret.txt"),
        no_index,
        dir_index,
        link_index,
    ] {
        assert!(WebRoot::open(&bad).is_err(), "{bad:?} was accepted");
    }
}

/// One trap planted in a clean tree: `(what, plant)`.
type Trap = (&'static str, fn(&Path, &Path));

/// Each trap, alone in a clean tree, stops the start (W2).
const TRAPS: &[Trap] = &[
    ("a symlink out of the root", |root, _| {
        symlink("../../secret.txt", &root.join("fonts/escape.txt"));
    }),
    ("an absolute symlink", |root, base| {
        symlink(
            base.join("secret.js").to_str().unwrap(),
            &root.join("_next/static/abs.js"),
        );
    }),
    ("a directory symlink", |root, _| {
        symlink("../../..", &root.join("_next/static/up"));
    }),
    ("a symlink inside the root", |root, _| {
        symlink("OFL.txt", &root.join("fonts/inside.txt"));
    }),
    ("fonts/ as a symlink", |root, _| {
        fs::rename(root.join("fonts"), root.join("fonts-real")).unwrap();
        symlink("fonts-real", &root.join("fonts"));
    }),
    ("_next/ as a symlink", |root, _| {
        fs::rename(root.join("_next"), root.join("next-real")).unwrap();
        symlink("next-real", &root.join("_next"));
    }),
    ("a top-level symlink", |root, _| {
        symlink("..", &root.join("up"));
    }),
    ("a FIFO", |root, _| mkfifo(&root.join("fonts/pipe.txt"))),
    ("a dotfile", |root, _| {
        fs::write(root.join("fonts/.hidden.txt"), b"x").unwrap();
    }),
    (".DS_Store", |root, _| {
        fs::write(root.join(".DS_Store"), b"x").unwrap();
    }),
    ("a dot directory", |root, _| {
        fs::create_dir_all(root.join("fonts/.git")).unwrap();
        fs::write(root.join("fonts/.git/x.txt"), b"x").unwrap();
    }),
    ("a source map", |root, _| {
        fs::write(root.join("_next/static/chunks/main-0a1b2c3d.js.map"), b"{}").unwrap();
    }),
    ("a JSON file", |root, _| {
        fs::write(root.join("_next/static/chunks/data.json"), b"{}").unwrap();
    }),
    ("an upper-case extension", |root, _| {
        fs::write(root.join("fonts/UPPER.TXT"), b"x").unwrap();
    }),
    ("no extension", |root, _| {
        fs::write(root.join("fonts/LICENSE"), b"x").unwrap();
    }),
    ("a space in a name", |root, _| {
        fs::write(root.join("fonts/a b.txt"), b"x").unwrap();
    }),
    ("a percent in a name", |root, _| {
        fs::write(root.join("fonts/a%2e.txt"), b"x").unwrap();
    }),
    ("a non-ASCII name", |root, _| {
        fs::write(root.join("fonts/\u{e9}.txt"), b"x").unwrap();
    }),
    ("a file over the cap", |root, _| {
        let big = usize::try_from(MAX_FILE_BYTES).unwrap() + 1;
        fs::write(root.join("_next/static/big-1.js"), vec![b'x'; big]).unwrap();
    }),
    ("too many files", |root, _| {
        for n in 0..MAX_WEB_FILES {
            fs::write(root.join(format!("_next/static/f-{n}.js")), b"x").unwrap();
        }
    }),
    ("too many bytes in all", |root, _| {
        let each = usize::try_from(MAX_FILE_BYTES).unwrap();
        for n in 0..=(MAX_WEB_BYTES / MAX_FILE_BYTES) {
            fs::write(
                root.join(format!("_next/static/m-{n}.js")),
                vec![b'x'; each],
            )
            .unwrap();
        }
    }),
    ("a hard link to a file outside", |root, base| {
        fs::hard_link(base.join("secret.txt"), root.join("fonts/linked.txt")).unwrap();
    }),
    ("a hard link inside", |root, _| {
        fs::hard_link(root.join("fonts/OFL.txt"), root.join("fonts/OFL2.txt")).unwrap();
    }),
];

#[test]
fn w2_every_trap_in_the_root_stops_the_start() {
    for (n, (what, plant)) in TRAPS.iter().enumerate() {
        let t = tree(&format!("w2_trap_{n}"));
        plant(&t.root, t.root.parent().unwrap());
        assert!(
            WebRoot::open(&t.root).is_err(),
            "{what}: the root was accepted"
        );
    }
}

#[test]
fn w2_the_binary_checks_the_web_root_before_it_starts() {
    let t = tree("w2_binary");
    let program = server_binary();
    let base = t.root.parent().unwrap();
    // Each run must end by itself; one still running after 10 s (it
    // started serving) is stopped and fails the test, never hangs it.
    let run = |args: &[&str]| {
        let mut child = Command::new(&program)
            .args(args)
            .env_clear()
            .env("PATH", base)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("run openwiz-server");
        let until = Instant::now() + Duration::from_secs(10);
        while child.try_wait().expect("wait").is_none() {
            if Instant::now() > until {
                let _ = child.kill();
                let _ = child.wait();
                panic!("openwiz-server {args:?} did not exit: it started");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        child.wait_with_output().expect("output")
    };
    let missing = base.join("missing-dir-name");
    let out = run(&["--port", "0", "--web-root", missing.to_str().unwrap()]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(
        !stderr.contains("missing-dir-name"),
        "the path is echoed: {stderr}"
    );
    assert!(stderr.contains("--web-root"), "{stderr}");

    // A root with a trap: refused, with the rule, not the path.
    let trapped = tree("w2_binary_trap");
    symlink("../../secret.txt", &trapped.root.join("fonts/escape.txt"));
    let out = run(&["--port", "0", "--web-root", trapped.root.to_str().unwrap()]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("symlink"), "{stderr}");
    assert!(
        !stderr.contains("escape") && !stderr.contains("w2_binary_trap"),
        "{stderr}"
    );

    let root = t.root.to_str().unwrap();
    let out = run(&["--web-root", root, "--web-root", root]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let out = run(&["--web-root"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");

    let help = run(&["--help"]);
    assert!(
        String::from_utf8_lossy(&help.stdout).contains("--web-root DIR"),
        "--help does not document --web-root"
    );

    // A good root: it starts, and serves the page.
    let mut child = Command::new(&program)
        .args(["--port", "0", &format!("--web-root={root}")])
        .env_clear()
        .env("PATH", base)
        .stderr(Stdio::piped())
        .spawn()
        .expect("start openwiz-server");
    let mut line = String::new();
    BufReader::new(child.stderr.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let port: u16 = line
        .split("http://127.0.0.1:")
        .nth(1)
        .and_then(|rest| rest.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|p| p.parse().ok())
        .unwrap_or_else(|| panic!("no start line: {line:?}"));
    let r = crate::http::get(([127, 0, 0, 1], port).into(), "/");
    let _ = child.kill();
    let _ = child.wait();
    assert_eq!(r.status, 200, "{}", r.status_line);
    assert_eq!(r.body, t.files["/"]);
}

// W3 ---------------------------------------------------------------------

#[test]
fn w3_serves_exactly_the_allowlist() {
    let fake = Fake::new("web_a8_w3", "normal");
    let t = tree("w3");
    let s = serve_tree(&fake, &t);
    for (url, bytes) in &t.files {
        let r = s.get(url);
        assert_eq!(r.status, 200, "{url}: {}", r.status_line);
        assert_eq!(&r.body, bytes, "{url}: the bytes");
    }
    for url in [
        "/index.html",
        "/404.html",
        "/robots.txt",
        "/favicon.ico",
        "/_next",
        "/_next/",
        "/_next/BUILDID_syn-1",
        "/_next/BUILDID_syn-1/",
        "/_next/static",
        "/_next/static/",
        "/_next/static/chunks",
        "/_next/static/chunks/",
        "/_next/static/chunks/main-0a1b2c3d.js.map",
        "/_next/static/chunks/data.json",
        "/_next/static/chunks/missing-1.js",
        "/_next/STATIC/chunks/main-0a1b2c3d.js",
        "/fonts",
        "/fonts/",
        "/fonts/UPPER.TXT",
        "/fonts/missing.woff2",
        "/FONTS/OFL.txt",
        "/static/chunks/main-0a1b2c3d.js",
        "/chunks/main-0a1b2c3d.js",
        "/out/index.html",
    ] {
        assert_not_served(url, &s.get(url));
    }
    assert!(fake.invocations().is_empty());
}

// W4 ---------------------------------------------------------------------

#[test]
fn w4_traversal_and_encodings_are_refused() {
    let fake = Fake::new("web_a8_w4", "normal");
    let t = tree("w4");
    let s = serve_tree(&fake, &t);
    for url in [
        // Traversal.
        "/fonts/../index.html",
        "/fonts/../../secret.txt",
        "/fonts/../../outside/fonts/OFL.txt",
        "/_next/static/../../../secret.js",
        "/_next/static/chunks/../../../../secret.js",
        "/fonts/./OFL.txt",
        "/fonts//OFL.txt",
        "/fonts/OFL.txt/",
        "/fonts/OFL.txt/.",
        "/fonts/...txt",
        // Encoded traversal: nothing is percent-decoded.
        "/fonts/%2e%2e/%2e%2e/secret.txt",
        "/fonts/%2E%2E%2Fsecret.txt",
        "/fonts/..%2f..%2fsecret.txt",
        "/fonts/..%5c..%5csecret.txt",
        "/fonts/%252e%252e/secret.txt",
        "/fonts/OFL%2etxt",
        "/fonts/OFL.txt%00",
        "/%5f%6e%65%78%74/static/chunks/main-0a1b2c3d.js",
        "/fonts/%c0%ae%c0%ae/secret.txt",
        // Other bytes outside the allowed set.
        "/fonts/..\\..\\secret.txt",
        "/fonts/OFL.txt;x",
        "/fonts/OFL.txt~",
        "/fonts/OFL+.txt",
        "/fonts/O:FL.txt",
        "/fonts/*.txt",
    ] {
        assert_not_served(url, &s.get(url));
    }
    assert_eq!(s.get("/fonts/OFL.txt").status, 200);
    assert!(fake.invocations().is_empty());
}

// W5 ---------------------------------------------------------------------

#[test]
fn w5_only_get_and_head_and_no_query_or_body() {
    let fake = Fake::new("web_a8_w5", "normal");
    let t = tree("w5");
    let s = serve_tree(&fake, &t);
    for url in t.files.keys() {
        for method in [
            "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "TRACE", "PROPFIND", "MKCOL", "LOCK",
        ] {
            let r = s.request(method, url);
            assert_eq!(r.status, 405, "{method} {url}");
            assert_eq!(r.error().0, "method_not_allowed", "{method} {url}");
            assert_eq!(r.header("Allow"), Some("GET, HEAD"), "{method} {url}");
        }
        let get = s.get(url);
        let head = s.request("HEAD", url);
        assert_eq!(head.status, 200, "HEAD {url}");
        assert!(head.body.is_empty(), "HEAD {url} has a body");
        assert_eq!(head.header("Content-Length"), None, "HEAD {url}");
        assert_page_headers(&format!("HEAD {url}"), url, &head, true);
        assert_page_headers(&format!("GET {url}"), url, &get, false);
    }
    // A refusal to HEAD has no body either.
    let r = s.request("HEAD", "/fonts/../../secret.txt");
    assert_eq!(r.status, 404);
    assert!(r.body.is_empty());
    // HEAD on the API is still refused, with the API's Allow.
    for url in ["/api/v1/health", "/api/v1/inventory", "/api/v1/nope"] {
        let r = s.request("HEAD", url);
        assert_eq!(r.status, 405, "HEAD {url}");
        assert_eq!(r.header("Allow"), Some("GET"), "HEAD {url}");
    }
    // Web paths take no query.
    for target in [
        "/?",
        "/?x=1",
        "/fonts/OFL.txt?v=1",
        "/_next/static/chunks/main-0a1b2c3d.js?",
    ] {
        let r = s.get(target);
        assert_eq!(r.status, 400, "{target}");
        assert_eq!(r.error().0, "invalid_request", "{target}");
    }
    // No request body.
    let host = s.host();
    for extra in ["Content-Length: 5\r\n", "Transfer-Encoding: chunked\r\n"] {
        let r = s.send(format!("GET / HTTP/1.1\r\nHost: {host}\r\n{extra}\r\nhello").as_bytes());
        assert_eq!(r.status, 400, "{extra:?}");
        assert_eq!(r.error().0, "bad_request", "{extra:?}");
    }
    assert!(fake.invocations().is_empty());
}

// W6, W7 -----------------------------------------------------------------

#[test]
fn w6_w7_content_types_and_page_headers() {
    let fake = Fake::new("web_a8_w7", "normal");
    let t = tree("w7");
    let s = serve_tree(&fake, &t);
    for url in t.files.keys() {
        assert_page_headers(url, url, &s.get(url), false);
    }
}

#[test]
fn w7_the_page_csp_is_6s_and_runs_no_inline_script() {
    let directives: Vec<&str> = PAGE_CSP.split("; ").collect();
    assert!(directives.contains(&"script-src 'self'"), "{PAGE_CSP}");
    assert!(directives.contains(&"style-src 'self'"), "{PAGE_CSP}");
    assert!(directives.contains(&"default-src 'none'"), "{PAGE_CSP}");
    assert!(directives.contains(&"frame-ancestors 'none'"), "{PAGE_CSP}");
    for banned in [
        "unsafe", "nonce-", "sha256-", "sha384-", "sha512-", "*", "data:", "http",
    ] {
        assert!(!PAGE_CSP.contains(banned), "{banned} in {PAGE_CSP}");
    }
    // The same policy the web page's own proofs load it under.
    let mjs = fs::read_to_string(WEB_SERVE_MJS).expect("apps/web/test/support/serve.mjs");
    let list = mjs
        .split("export const PAGE_CSP = [")
        .nth(1)
        .and_then(|rest| rest.split("].join('; ')").next())
        .expect("PAGE_CSP in serve.mjs");
    let theirs: Vec<String> = list
        .split(',')
        .map(|item| item.trim().trim_matches('"').to_owned())
        .filter(|item| !item.is_empty())
        .collect();
    assert_eq!(
        directives
            .iter()
            .map(|d| (*d).to_owned())
            .collect::<Vec<_>>(),
        theirs,
        "PAGE_CSP differs from apps/web's"
    );
    // The caching default (see `STATIC_CACHE`).
    assert_eq!(STATIC_CACHE, "private, max-age=31536000, immutable");
}

// W8, W9 (the real export) -------------------------------------------------

/// Every file under `dir`, relative, with `/` separators, sorted.
fn files_under(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in fs::read_dir(&d).expect("read dir") {
            let path = entry.expect("entry").path();
            let kind = fs::symlink_metadata(&path).expect("lstat").file_type();
            assert!(!kind.is_symlink(), "a symlink in the export: {path:?}");
            if kind.is_dir() {
                stack.push(path);
            } else {
                let rel = path.strip_prefix(dir).unwrap();
                out.push(rel.to_str().unwrap().replace('\\', "/"));
            }
        }
    }
    out.sort();
    out
}

fn real_export() -> PathBuf {
    let out = PathBuf::from(WEB_OUT);
    assert!(
        out.join("index.html").is_file(),
        "A8's real-export proofs need apps/web/out: run `cd apps/web && npm ci --ignore-scripts && npm run build` first"
    );
    out
}

/// The export's files that are deliberately not served.
const NOT_SERVED: &[&str] = &["404.html"];

#[test]
fn w8_the_real_export_is_served_byte_for_byte() {
    let out = real_export();
    let fake = Fake::new("web_a8_w8", "normal");
    let s = serve_with(&fake, web_config(&out));
    let mut served = 0;
    for rel in files_under(&out) {
        if NOT_SERVED.contains(&rel.as_str()) {
            assert_not_served(&rel, &s.get(&format!("/{rel}")));
            continue;
        }
        let url = if rel == "index.html" {
            "/".to_owned()
        } else {
            assert!(
                rel.starts_with("_next/static/") || rel.starts_with("fonts/"),
                "the export emits {rel}, which is neither served nor on NOT_SERVED"
            );
            format!("/{rel}")
        };
        let r = s.get(&url);
        assert_eq!(r.status, 200, "{url}: {}", r.status_line);
        assert_eq!(
            r.body,
            fs::read(out.join(&rel)).unwrap(),
            "{url}: the bytes"
        );
        assert_page_headers(&url, &url, &r, false);
        served += 1;
    }
    assert!(served >= 5, "only {served} files served");
    assert_not_served("/index.html", &s.get("/index.html"));
    assert!(fake.invocations().is_empty());
}

/// The attributes of one start tag (`<name …>`), lower-cased names.
fn attributes(tag: &str) -> Vec<(String, String)> {
    let bytes = tag.as_bytes();
    let mut i = 1;
    while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' {
        i += 1;
    }
    let mut out = Vec::new();
    loop {
        while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b'/') {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] == b'>' {
            return out;
        }
        let start = i;
        while i < bytes.len() && !b" \t\r\n=>/".contains(&bytes[i]) {
            i += 1;
        }
        let name = tag[start..i].to_ascii_lowercase();
        let mut value = String::new();
        if i < bytes.len() && bytes[i] == b'=' {
            i += 1;
            if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
                let quote = bytes[i];
                let from = i + 1;
                i = from;
                while i < bytes.len() && bytes[i] != quote {
                    i += 1;
                }
                value = tag[from..i.min(bytes.len())].to_owned();
                i += 1;
            } else {
                let from = i;
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' {
                    i += 1;
                }
                value = tag[from..i].to_owned();
            }
        }
        out.push((name, value));
    }
}

/// W9's rules for one HTML document; the problems found.
fn inline_problems(html: &str) -> Vec<String> {
    let mut problems = Vec::new();
    if html.to_ascii_lowercase().contains("javascript:") {
        problems.push("javascript: URL".to_owned());
    }
    let mut rest = html;
    while let Some(at) = rest.find('<') {
        rest = &rest[at..];
        let end = rest.find('>').map_or(rest.len(), |e| e + 1);
        let tag = &rest[..end];
        let name: String = tag[1..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        let attrs = attributes(tag);
        for (attr, _) in &attrs {
            if attr.starts_with("on") {
                problems.push(format!("<{name}> has an {attr} handler"));
            }
            if attr == "style" {
                problems.push(format!("<{name}> has a style attribute"));
            }
        }
        if name == "style" {
            problems.push("a <style> element".to_owned());
        }
        rest = &rest[end..];
        if name == "script" {
            let close = rest
                .to_ascii_lowercase()
                .find("</script")
                .unwrap_or(rest.len());
            let body = &rest[..close];
            let get = |n: &str| attrs.iter().find(|(a, _)| a == n).map(|(_, v)| v.as_str());
            match (get("src"), get("type")) {
                (Some(src), _) if src.starts_with('/') && !src.starts_with("//") => {
                    if !body.trim().is_empty() {
                        problems.push(format!("<script src={src}> has inline text"));
                    }
                }
                (None, Some("application/json")) => {}
                (src, kind) => {
                    problems.push(format!("an inline or foreign <script> ({src:?}, {kind:?})"))
                }
            }
            rest = &rest[close..];
        }
    }
    problems
}

#[test]
fn w9_the_checker_finds_inline_script() {
    for bad in [
        "<script>alert(1)</script>",
        "<script type=\"module\">x()</script>",
        "<script src=\"https://example.invalid/x.js\"></script>",
        "<script src=\"//example.invalid/x.js\"></script>",
        "<script src=\"/a.js\">x()</script>",
        "<img src=\"/a\" onerror=\"x()\">",
        "<body ONLOAD=x()>",
        "<a href=\"javascript:x()\">a</a>",
        "<style>body{}</style>",
        "<p style=\"color:red\">a</p>",
    ] {
        assert!(!inline_problems(bad).is_empty(), "missed: {bad}");
    }
    assert!(
        inline_problems(
            "<script src=\"/_next/static/a.js\" defer=\"\"></script>\
             <script id=\"__NEXT_DATA__\" type=\"application/json\">{\"a\":1}</script>"
        )
        .is_empty()
    );
}

#[test]
fn w9_the_real_export_has_no_inline_script() {
    let out = real_export();
    let mut pages = 0;
    for rel in files_under(&out) {
        if !rel.ends_with(".html") {
            continue;
        }
        let html = fs::read_to_string(out.join(&rel)).unwrap();
        let problems = inline_problems(&html);
        assert!(problems.is_empty(), "{rel}: {problems:#?}");
        assert!(
            html.contains("<script src=\"/_next/static/"),
            "{rel}: no scripts?"
        );
        pages += 1;
    }
    assert!(pages >= 2, "{pages} pages");
}

// W11 ---------------------------------------------------------------------

/// `GET target`, or `None` without a whole response within `within`.
fn get_within(addr: SocketAddr, target: &str, within: Duration) -> Option<Response> {
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream.set_read_timeout(Some(within)).unwrap();
    let raw = format!(
        "GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",
        addr.port()
    );
    stream.write_all(raw.as_bytes()).ok()?;
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => bytes.extend_from_slice(&chunk[..n]),
            Err(_) => return None,
        }
    }
    (!bytes.is_empty()).then(|| crate::http::parse_response(&bytes))
}

#[test]
fn w11_a_file_replaced_by_a_fifo_after_start_is_answered_at_once() {
    let fake = Fake::new("web_a8_w11_fifo", "normal");
    let t = tree("w11_fifo");
    let s = serve_tree(&fake, &t);
    for url in ["/fonts/OFL.txt", "/"] {
        let rel = if url == "/" { "index.html" } else { &url[1..] };
        let path = t.root.join(rel);
        fs::remove_file(&path).unwrap();
        mkfifo(&path);
        let r = get_within(s.addr, url, Duration::from_secs(5))
            .unwrap_or_else(|| panic!("{url}: no answer within 5 s (a request opened the FIFO)"));
        assert_eq!(r.status, 200, "{url}");
        assert_eq!(r.body, t.files[url], "{url}: the bytes loaded at start");
    }
}

#[test]
fn w11_the_root_replaced_changed_or_deleted_after_start_changes_nothing() {
    let fake = Fake::new("web_a8_w11_root", "normal");
    let t = tree("w11_root");
    let s = serve_tree(&fake, &t);
    let base = t.root.parent().unwrap();
    let check = |when: &str| {
        for (url, bytes) in &t.files {
            let r = s.get(url);
            assert_eq!(r.status, 200, "{when}: {url}");
            assert_eq!(&r.body, bytes, "{when}: {url}: the bytes loaded at start");
        }
        for url in ["/fonts/new.txt", "/fonts/secret.txt"] {
            assert_not_served(&format!("{when}: {url}"), &s.get(url));
        }
    };
    // A reproduction: the root renamed, its pathname now a symlink to
    // a directory outside with its own index.html and fonts/.
    let old = base.join("out-old");
    fs::rename(&t.root, &old).unwrap();
    symlink("outside", &t.root);
    check("root replaced");
    // A served file changed, and a new one added, in the real directory.
    fs::write(old.join("_next/static/chunks/main-0a1b2c3d.js"), SECRET).unwrap();
    fs::write(old.join("fonts/new.txt"), SECRET).unwrap();
    fs::remove_file(&t.root).unwrap();
    fs::rename(&old, &t.root).unwrap();
    check("changed");
    // The whole root deleted.
    fs::remove_dir_all(&t.root).unwrap();
    check("root deleted");
}

// W10 --------------------------------------------------------------------

#[test]
fn w10_the_api_is_unaffected_and_web_paths_obey_host() {
    let fake = Fake::new("web_a8_w10", "normal");
    let t = tree("w10");
    let plain = serve(&fake);
    let web = serve_tree(&fake, &t);
    for target in [
        "/api/v1/health",
        "/api/v1/inventory",
        "/api/v1/inventory?detail=2&hidden=1",
        "/api/v1/sessions/thr_syn_0001",
        "/api/v1/sessions/thr_syn_9999",
        "/api/v1/inventory?bad=1",
        "/api/v1/nope",
        "/api/v1/../../fonts/OFL.txt",
    ] {
        let (a, b) = (plain.get(target), web.get(target));
        assert_eq!(a.status, b.status, "{target}");
        assert_eq!(a.headers, b.headers, "{target}");
        assert_eq!(a.body, b.body, "{target}");
    }
    for method in ["POST", "HEAD", "DELETE"] {
        let r = web.request(method, "/api/v1/inventory");
        assert_eq!(r.status, 405, "{method}");
        assert_eq!(r.header("Allow"), Some("GET"), "{method}");
    }
    // The Host allowlist comes first on web paths too.
    for url in ["/", "/fonts/OFL.txt"] {
        let r = web
            .send(format!("GET {url} HTTP/1.1\r\nHost: other.example.invalid\r\n\r\n").as_bytes());
        assert_eq!(r.status, 421, "{url}");
        assert_api_headers(url, &r);
    }
    let named = serve_with(
        &fake,
        Config {
            allowed_host: Some("openwiz.example.invalid".to_owned()),
            ..web_config(&t.root)
        },
    );
    let r = named.send(b"GET /fonts/OFL.txt HTTP/1.1\r\nHost: openwiz.example.invalid\r\n\r\n");
    assert_eq!(r.status, 200);
    assert_eq!(r.body, t.files["/fonts/OFL.txt"]);
    // The log names the route template only, never the file or query.
    let before = web.log_text().lines().count();
    for target in [
        "/",
        "/_next/static/chunks/main-0a1b2c3d.js",
        "/fonts/OFL.txt",
        "/fonts/Secret-name.txt",
        "/fonts/OFL.txt?v=quiet",
    ] {
        let _ = web.get(target);
    }
    // The server logs after it has answered and closed: wait for the lines.
    let until = Instant::now() + Duration::from_secs(5);
    while web.log_text().lines().count() < before + 5 && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    let log = web.log_text();
    let lines: Vec<&str> = log.lines().skip(before).collect();
    let routes: Vec<String> = lines
        .iter()
        .map(|l| {
            l.rsplitn(3, ' ')
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .take(2)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    assert_eq!(
        routes,
        [
            "GET / 200",
            "GET /_next/static/* 200",
            "GET /fonts/* 200",
            "GET /fonts/* 404",
            "GET /fonts/* 400",
        ],
        "{log}"
    );
    for word in [
        "main-0a1b2c3d",
        "OFL",
        "Secret-name",
        "quiet",
        ".js",
        ".txt",
    ] {
        assert!(!log.contains(word), "the log names {word}: {log}");
    }
    assert_eq!(WEB_ROUTES, &["/", "/_next/static/*", "/fonts/*"]);
}
