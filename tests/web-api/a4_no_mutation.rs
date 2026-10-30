//! A4: no mutation surface. Only `GET` on the three allowlisted routes; every
//! other method is `405` and every other path `404`, with no read. The
//! server calls only the core's list and show, and makes exactly the reads
//! the CLI makes. P8(a) (in `tests/proof`) scans `crates/server` too.

use std::collections::BTreeMap;

use std::io::Write;
use std::net::TcpStream;
use std::time::Duration;

use openwiz_server::{Config, Limits, ROUTES, Route};

use crate::fake::Fake;
use crate::http::{Response, clear_invocations, read_response, serve, serve_with};

const SERVER_SRC: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../crates/server/src");

fn server_sources() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(SERVER_SRC).expect("server sources") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|e| e == "rs") {
            out.push((
                path.display().to_string(),
                std::fs::read_to_string(&path).expect("source"),
            ));
        }
    }
    assert!(!out.is_empty());
    out
}

#[test]
fn the_route_table_is_exactly_three_get_routes() {
    assert_eq!(
        ROUTES,
        &[
            Route {
                method: "GET",
                path: "/api/v1/inventory",
            },
            Route {
                method: "GET",
                path: "/api/v1/sessions/{id}",
            },
            Route {
                method: "GET",
                path: "/api/v1/health",
            },
        ][..]
    );
}

#[test]
fn every_other_method_is_405_and_reads_nothing() {
    let fake = Fake::new("web_a4_methods", "normal");
    let s = serve(&fake);
    for method in [
        "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "CONNECT", "TRACE", "PROPFIND",
        "MKCOL", "LINK", "get", "Get", "GETS", "X",
    ] {
        for target in [
            "/api/v1/inventory",
            "/api/v1/sessions/thr_syn_0001",
            "/api/v1/health",
            "/nowhere",
        ] {
            let r = s.request(method, target);
            assert_eq!(r.status, 405, "{method} {target}: {}", r.status_line);
            assert_eq!(r.header("Allow"), Some("GET"), "{method} {target}");
            // A response to HEAD has no body; its own test checks it.
            if method != "HEAD" {
                assert_eq!(r.error().0, "method_not_allowed");
            }
        }
    }
    // A body on a refused method is never read as a request.
    let r = s.send(
        format!(
            "POST /api/v1/inventory HTTP/1.1\r\nHost: {}\r\nContent-Length: 2\r\n\r\n{{}}",
            s.host()
        )
        .as_bytes(),
    );
    assert_eq!(r.status, 405);
    assert!(fake.invocations().is_empty(), "{:?}", fake.invocations());
}

/// HTTP forbids content in a response to `HEAD`. `HEAD` is still
/// refused (`405`, `Allow: GET`), as is anything else, but with no body.
#[test]
fn a_head_request_is_refused_without_a_body() {
    let fake = Fake::new("web_a4_head", "normal");
    let s = serve(&fake);
    let host = s.host();
    let mut cases: Vec<(String, u16)> = [
        "/api/v1/inventory",
        "/api/v1/inventory?detail=2",
        "/api/v1/sessions/thr_syn_0001",
        "/api/v1/health",
        "/nowhere",
    ]
    .iter()
    .map(|target| {
        (
            format!("HEAD {target} HTTP/1.1\r\nHost: {host}\r\n\r\n"),
            405,
        )
    })
    .collect();
    // Refused before the method is looked at: still no body for HEAD.
    cases.push((
        "HEAD /api/v1/health HTTP/1.1\r\nHost: other.example.invalid\r\n\r\n".to_owned(),
        421,
    ));
    cases.push(("HEAD /api/v1/health HTTP/1.1\r\n\r\n".to_owned(), 400));
    for (raw, status) in cases {
        let r = s.send(raw.as_bytes());
        assert_eq!(r.status, status, "{raw:?}");
        assert!(
            r.body.is_empty(),
            "{raw:?}: a response to HEAD carries {} body bytes",
            r.body.len()
        );
        assert!(
            r.header("Content-Length").is_none(),
            "{raw:?}: Content-Length {:?}",
            r.header("Content-Length")
        );
        if status == 405 {
            assert_eq!(r.header("Allow"), Some("GET"), "{raw:?}");
        }
        assert_eq!(r.header("Cache-Control"), Some("no-store"), "{raw:?}");
        assert_eq!(r.header("Connection"), Some("close"), "{raw:?}");
    }
    assert!(fake.invocations().is_empty(), "{:?}", fake.invocations());
}

fn assert_no_body(what: &str, r: &Response, status: u16) {
    assert_eq!(r.status, status, "{what}");
    assert!(
        r.body.is_empty(),
        "{what}: a response to HEAD carries {} body bytes",
        r.body.len()
    );
    assert!(
        r.header("Content-Length").is_none(),
        "{what}: Content-Length {:?}",
        r.header("Content-Length")
    );
}

/// A HEAD request refused before its head parses still has no body, since
/// its request line already says HEAD:
/// `414`, `431`, `505`, a malformed request line (`400`), a head that
/// arrives too late (`408`) and the connection cap's `503`.
#[test]
fn a_head_request_refused_before_its_head_parses_has_no_body() {
    let fake = Fake::new("web_a4_head_early", "normal");
    let s = serve_with(
        &fake,
        Config {
            allowed_host: None,
            web_root: None,
            limits: Limits {
                head_timeout: Duration::from_millis(800),
                ..Limits::default()
            },
        },
    );
    let host = s.host();
    let many: String = (0..80).map(|i| format!("X-Synthetic-{i}: 1\r\n")).collect();
    for (what, raw, status) in [
        (
            "505",
            format!("HEAD /api/v1/health HTTP/2.0\r\nHost: {host}\r\n\r\n"),
            505,
        ),
        (
            "414",
            format!(
                "HEAD /{} HTTP/1.1\r\nHost: {host}\r\n\r\n",
                "a".repeat(2000)
            ),
            414,
        ),
        (
            "431",
            format!("HEAD /api/v1/health HTTP/1.1\r\nHost: {host}\r\n{many}\r\n"),
            431,
        ),
        ("400", "HEAD /api/v1/health\r\n\r\n".to_owned(), 400),
    ] {
        assert_no_body(what, &s.send(raw.as_bytes()), status);
    }
    // Half a HEAD request, then nothing: 408, still no body.
    let mut slow = TcpStream::connect(s.addr).unwrap();
    slow.set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    slow.write_all(b"HEAD /api/v1/he").unwrap();
    assert_no_body("408", &read_response(&mut slow), 408);
    assert!(fake.invocations().is_empty());

    // Over the connection cap: 503, still no body for HEAD.
    let capped = serve_with(
        &fake,
        Config {
            allowed_host: None,
            web_root: None,
            limits: Limits {
                max_connections: 1,
                head_timeout: Duration::from_secs(5),
                ..Limits::default()
            },
        },
    );
    let _idle = TcpStream::connect(capped.addr).unwrap();
    capped.wait_for("one connection", |st| st.connections == 1);
    let r = capped.request("HEAD", "/api/v1/health");
    assert_no_body("503", &r, 503);
    assert_eq!(r.header("Retry-After"), Some("5"));

    // The method arrives in pieces, slower than the server peeks.
    // A beginning of `HEAD ` may be HEAD, so the 503 still has no body.
    let host = capped.host();
    for first in ["H", "HEA", "HEAD"] {
        let mut split = TcpStream::connect(capped.addr).unwrap();
        split
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        split.write_all(first.as_bytes()).unwrap();
        std::thread::sleep(Duration::from_millis(400));
        let rest = format!("HEAD /api/v1/health HTTP/1.1\r\nHost: {host}\r\n\r\n");
        let _ = split.write_all(&rest.as_bytes()[first.len()..]);
        assert_no_body(
            &format!("503 after {first:?}"),
            &read_response(&mut split),
            503,
        );
    }
    // Under the cap too: a head that stops inside the method is 408 with no
    // body when what arrived may be HEAD.
    let mut cut = TcpStream::connect(s.addr).unwrap();
    cut.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    cut.write_all(b"HEA").unwrap();
    assert_no_body("408 after \"HEA\"", &read_response(&mut cut), 408);
}

#[test]
fn every_other_path_is_404_and_reads_nothing() {
    let fake = Fake::new("web_a4_paths", "normal");
    let s = serve(&fake);
    for target in [
        "/",
        "/index.html",
        "/favicon.ico",
        "/static/app.js",
        "/api",
        "/api/",
        "/api/v1",
        "/api/v1/",
        "/api/v2/inventory",
        "/API/V1/INVENTORY",
        "/api/v1/inventory/",
        "/api/v1/inventory/thr_syn_0001",
        "/api/v1/inventory.json",
        "/api/v1/health/",
        "/api/v1/sessions",
        "/api/v1/sessions/",
        "/api/v1/sessions/thr_syn_0001/",
        "/api/v1/sessions/thr_syn_0001/output",
        "/api/v1/sessions/thr_syn_0001/messages",
        "/api/v1/sessions/thr_syn_0001/launch",
        "/api/v1/launch",
        "/api/v1/messages",
        "/api/v1/send",
        "/api/v1/sessions/thr_syn_0001/send",
        "/api/v1/sessions/thr_syn_0001/tell",
        "/api/v1/approve",
        "/api/v1/ws",
        "/api/v1/events",
        "//api/v1/inventory",
        "/api//v1/inventory",
        "/api/v1/./inventory",
        "/api/v1/../v1/inventory",
        "/../api/v1/inventory",
        "/%61pi/v1/inventory",
        "/api/v1/%69nventory",
        "/api/v1/inventory;x",
    ] {
        let r = s.get(target);
        assert_eq!(r.status, 404, "{target}: {}", r.text());
        assert_eq!(r.error().0, "no_route", "{target}");
    }
    assert!(fake.invocations().is_empty(), "{:?}", fake.invocations());
}

#[test]
fn health_reads_nothing() {
    let fake = Fake::new("web_a4_health", "version-0.44.0");
    let s = serve(&fake);
    let r = s.get("/api/v1/health");
    assert_eq!(r.status, 200, "{}", r.text());
    let body = r.json();
    assert_eq!(body["server"]["name"], "openwiz-server");
    assert_eq!(body["server"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(body["source"]["name"], "room-cli");
    assert_eq!(body["source"]["tested_version"], "v127");
    assert_eq!(
        body.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["server", "source"]
    );
    assert!(
        fake.invocations().is_empty(),
        "health must not read room-cli"
    );
}

#[test]
fn an_upgrade_request_is_never_upgraded() {
    let fake = Fake::new("web_a4_upgrade", "normal");
    let s = serve(&fake);
    let r = s.send(
        format!(
            "GET /api/v1/health HTTP/1.1\r\nHost: {}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\
             Sec-WebSocket-Key: c3ludGhldGljIGtleSAwMQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
            s.host()
        )
        .as_bytes(),
    );
    assert_eq!(r.status, 200);
    assert_eq!(r.header("Upgrade"), None);
    assert_eq!(r.header("Connection"), Some("close"));
}

#[test]
fn the_server_calls_only_the_cores_list_and_show() {
    let sources = server_sources();
    let all: String = sources.iter().map(|(_, t)| t.as_str()).collect();
    assert!(all.contains("ops::list_report("), "the server calls list");
    assert!(all.contains("ops::show_report("), "the server calls show");
    for (path, text) in &sources {
        for (i, _) in text.match_indices("ops::") {
            let rest = &text[i + "ops::".len()..];
            let names: Vec<String> = match rest.strip_prefix('{') {
                Some(list) => list[..list.find('}').expect("closing brace")]
                    .split(',')
                    .map(|n| n.trim().to_owned())
                    .filter(|n| !n.is_empty())
                    .collect(),
                None => vec![
                    rest.chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect(),
                ],
            };
            for name in names {
                assert!(
                    [
                        "self",
                        "list_report",
                        "show_report",
                        "Clock",
                        "ListRequest",
                        "ShowRequest",
                        "Detail",
                        "OpError",
                        "Report",
                    ]
                    .contains(&name.as_str()),
                    "{path}: ops::{name} is not list_report, show_report or their types"
                );
            }
        }
        assert!(
            !text.contains("ops::list(") && !text.contains("ops::show("),
            "{path}: only the report forms, as the CLI uses"
        );
        for forbidden in [
            "process::Command",
            "Command::new",
            "allowlist",
            "Invocation",
            "\"room-cli\"",
            "from_path()",
            "std::fs",
            "File::open",
            "read_dir",
            // No write path of any kind.
            "messaging",
            "openwiz_store",
            "Messenger",
            "SendStore",
            "ThreadTell",
        ] {
            let allowed_in_main = forbidden == "from_path()" && path.ends_with("main.rs");
            // A8: only web.rs reads files: the web page's, once, at
            // start (checked below: never while answering a request).
            let allowed_in_web = ["std::fs", "File::open", "read_dir"].contains(&forbidden)
                && path.ends_with("web.rs");
            assert!(
                !text.contains(forbidden) || allowed_in_main || allowed_in_web,
                "{path} contains {forbidden}"
            );
        }
    }
    // The code that answers a web request (from `route` on) touches no
    // file; the export is in memory since the start.
    let web = sources
        .iter()
        .find(|(p, _)| p.ends_with("web.rs"))
        .expect("web.rs");
    let requests = web
        .1
        .split("pub(crate) fn route(")
        .nth(1)
        .expect("web.rs answers requests from fn route on");
    for forbidden in ["fs::", "File", "read", "Path", "open", "Metadata"] {
        assert!(
            !requests.contains(forbidden),
            "web.rs's request code contains {forbidden}"
        );
    }
    let main = sources
        .iter()
        .find(|(p, _)| p.ends_with("main.rs"))
        .expect("main.rs");
    assert!(
        main.1.contains("CloudRoomSource::from_path()"),
        "the server builds its source as the CLI does"
    );
}

fn sorted(invocations: Vec<Vec<String>>) -> BTreeMap<Vec<String>, usize> {
    let mut counts = BTreeMap::new();
    for i in invocations {
        *counts.entry(i).or_insert(0) += 1;
    }
    counts
}

#[test]
fn the_server_makes_exactly_the_reads_the_cli_makes() {
    let fake = Fake::new("web_a4_same_reads", "normal");
    let s = serve(&fake);
    for (target, args) in [
        ("/api/v1/inventory", &["ls", "--json"][..]),
        ("/api/v1/inventory?detail=2", &["ls", "-l", "--json"][..]),
        (
            "/api/v1/inventory?detail=2&hidden=1&archived=1&limit=3&offset=1",
            &[
                "ls", "-l", "--hidden", "-a", "--limit", "3", "--offset", "1", "--json",
            ][..],
        ),
        (
            "/api/v1/sessions/thr_syn_0002",
            &["show", "thr_syn_0002", "--json"][..],
        ),
        (
            "/api/v1/sessions/thr_syn_9999",
            &["show", "thr_syn_9999", "--json"][..],
        ),
    ] {
        clear_invocations(&fake);
        let r = s.get(target);
        assert!(r.status == 200 || r.status == 404, "{target}: {}", r.text());
        let by_server = sorted(fake.invocations());
        clear_invocations(&fake);
        fake.cli(args);
        let by_cli = sorted(fake.invocations());
        assert!(!by_cli.is_empty());
        assert_eq!(
            by_server, by_cli,
            "{target} reads differ from openwiz {args:?}"
        );
    }
}

/// Normal (not dev or build) dependencies of `package`, from `cargo metadata`.
fn normal_dependencies(meta: &serde_json::Value, package: &str) -> Vec<String> {
    meta["packages"]
        .as_array()
        .expect("packages")
        .iter()
        .find(|p| p["name"] == package)
        .unwrap_or_else(|| panic!("no package {package}"))["dependencies"]
        .as_array()
        .expect("dependencies")
        .iter()
        .filter(|d| d["kind"].is_null())
        .map(|d| d["name"].as_str().expect("name").to_owned())
        .collect()
}

#[test]
fn the_server_depends_only_on_the_core_and_the_adapter_and_nothing_depends_on_it() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let out = std::process::Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--locked", "--offline"])
        .current_dir(&root)
        .output()
        .expect("run cargo metadata");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).expect("metadata JSON");
    let mut deps = normal_dependencies(&meta, "openwiz-server");
    deps.sort();
    assert_eq!(
        deps,
        [
            "openwiz-adapter-cloudroom",
            "openwiz-core",
            "serde",
            "serde_json"
        ]
    );
    for package in meta["packages"].as_array().expect("packages") {
        let name = package["name"].as_str().expect("name");
        assert!(
            !normal_dependencies(&meta, name).contains(&"openwiz-server".to_owned()),
            "{name} depends on openwiz-server"
        );
    }
    let core = normal_dependencies(&meta, "openwiz-core");
    let adapter = normal_dependencies(&meta, "openwiz-adapter-cloudroom");
    assert!(
        !core
            .iter()
            .chain(&adapter)
            .any(|d| d == "openwiz-server" || d == "openwiz-cli")
    );
}
