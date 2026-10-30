//! A7: nothing is cached or leaked. Every response, success or error, has
//! `no-store`, the security headers and no CORS header; errors carry no
//! paths, environment or stack traces; the log holds the method, route,
//! status and time only.

use std::io::{BufRead, BufReader, Read};
use std::net::SocketAddr;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use openwiz_server::{Config, Limits, ROUTES};

use crate::fake::Fake;
use crate::http::{self, Response, serve, serve_with};

fn assert_headers(what: &str, r: &Response) {
    assert_eq!(r.header("Content-Type"), Some("application/json"), "{what}");
    assert_eq!(r.header("Cache-Control"), Some("no-store"), "{what}");
    assert_eq!(
        r.header("X-Content-Type-Options"),
        Some("nosniff"),
        "{what}"
    );
    assert_eq!(r.header("Referrer-Policy"), Some("no-referrer"), "{what}");
    assert_eq!(r.header("X-Frame-Options"), Some("DENY"), "{what}");
    assert_eq!(
        r.header("Content-Security-Policy"),
        Some("default-src 'none'; frame-ancestors 'none'"),
        "{what}"
    );
    assert_eq!(r.header("Connection"), Some("close"), "{what}");
    assert_eq!(
        r.header("Content-Length"),
        Some(r.body.len().to_string().as_str()),
        "{what}"
    );
    for (name, _) in &r.headers {
        let lower = name.to_ascii_lowercase();
        assert!(
            !lower.starts_with("access-control-"),
            "{what}: CORS header {name}"
        );
        assert!(
            ![
                "set-cookie",
                "server",
                "x-powered-by",
                "etag",
                "last-modified"
            ]
            .contains(&lower.as_str()),
            "{what}: {name}"
        );
    }
    for name in [
        "Content-Type",
        "Cache-Control",
        "X-Content-Type-Options",
        "Connection",
    ] {
        assert_eq!(r.headers_named(name), 1, "{what}: {name} once");
    }
    let _ = r.json();
    assert!(r.status_line.starts_with("HTTP/1.1 "), "{what}");
}

#[test]
fn every_response_has_the_security_headers_and_no_cors() {
    let fake = Fake::new("web_a7_headers", "normal");
    let s = serve(&fake);
    let host = s.host();
    let origin = "Origin: https://other.example.invalid\r\n";
    let cases: Vec<(&str, String)> = vec![
        (
            "inventory",
            format!("GET /api/v1/inventory HTTP/1.1\r\nHost: {host}\r\n{origin}\r\n"),
        ),
        (
            "long",
            format!("GET /api/v1/inventory?detail=2 HTTP/1.1\r\nHost: {host}\r\n{origin}\r\n"),
        ),
        (
            "show",
            format!("GET /api/v1/sessions/thr_syn_0001 HTTP/1.1\r\nHost: {host}\r\n{origin}\r\n"),
        ),
        (
            "health",
            format!("GET /api/v1/health HTTP/1.1\r\nHost: {host}\r\n{origin}\r\n"),
        ),
        (
            "not found",
            format!("GET /api/v1/sessions/thr_syn_9999 HTTP/1.1\r\nHost: {host}\r\n\r\n"),
        ),
        (
            "bad query",
            format!("GET /api/v1/inventory?limit=x HTTP/1.1\r\nHost: {host}\r\n\r\n"),
        ),
        (
            "no route",
            format!("GET /nowhere HTTP/1.1\r\nHost: {host}\r\n\r\n"),
        ),
        (
            "preflight",
            format!(
                "OPTIONS /api/v1/inventory HTTP/1.1\r\nHost: {host}\r\n{origin}\
                 Access-Control-Request-Method: GET\r\n\r\n"
            ),
        ),
        (
            "post",
            format!("POST /api/v1/inventory HTTP/1.1\r\nHost: {host}\r\n\r\n"),
        ),
        (
            "foreign host",
            "GET /api/v1/health HTTP/1.1\r\nHost: other.example.invalid\r\n\r\n".to_owned(),
        ),
        ("no host", "GET /api/v1/health HTTP/1.1\r\n\r\n".to_owned()),
        (
            "http/2",
            format!("GET /api/v1/health HTTP/2.0\r\nHost: {host}\r\n\r\n"),
        ),
        (
            "long target",
            format!("GET /{} HTTP/1.1\r\nHost: {host}\r\n\r\n", "a".repeat(2000)),
        ),
        (
            "big head",
            format!(
                "GET /api/v1/health HTTP/1.1\r\nHost: {host}\r\nX-Big: {}\r\n\r\n",
                "b".repeat(20_000)
            ),
        ),
    ];
    for (what, raw) in cases {
        let r = s.send(raw.as_bytes());
        assert_headers(what, &r);
    }
    let r = s.send(b"GET /api/v1/health HTTP/1.1\r\nHost: 127.0.0.1:1\r\n\r\n");
    assert_eq!(r.status, 421);
    assert_headers("421", &r);

    let r = s.request("PUT", "/api/v1/health");
    assert_eq!(r.header("Allow"), Some("GET"));
}

#[test]
fn errors_from_the_core_have_the_same_headers() {
    for (scenario, target, status) in [
        ("thread-list-error", "/api/v1/inventory", 502),
        ("normal", "/api/v1/sessions/thr_syn_9999", 404),
        ("show-error", "/api/v1/sessions/thr_syn_0002", 502),
    ] {
        let fake = Fake::new(&format!("web_a7_core_{scenario}"), scenario);
        let s = serve(&fake);
        let r = s.get(target);
        assert_eq!(r.status, status, "{scenario}: {}", r.text());
        assert_headers(scenario, &r);
        let text = r.text();
        let dir = fake.dir.display().to_string();
        for leak in [
            dir.as_str(),
            env!("CARGO_MANIFEST_DIR"),
            "/home/",
            "/Users/",
            "/tmp/",
            "panicked",
            "RUST_BACKTRACE",
            "src/",
            ".rs:",
        ] {
            assert!(!text.contains(leak), "{scenario}: {leak} in {text}");
        }
        if let Ok(home) = std::env::var("HOME") {
            assert!(!text.contains(&home), "{scenario}: HOME in {text}");
        }
    }
}

#[test]
fn the_overload_answers_have_the_same_headers() {
    let fake = Fake::new("web_a7_overload", "normal");
    let s = serve_with(
        &fake,
        Config {
            allowed_host: None,
            web_root: None,
            limits: Limits {
                max_connections: 1,
                head_timeout: Duration::from_secs(2),
                ..Limits::default()
            },
        },
    );
    let mut idle = std::net::TcpStream::connect(s.addr).unwrap();
    s.wait_for("one connection", |st| st.connections == 1);
    let busy = s.get("/api/v1/health");
    assert_eq!(busy.status, 503);
    assert_headers("503", &busy);
    idle.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let timeout = http::read_response(&mut idle);
    assert_eq!(timeout.status, 408);
    assert_headers("408", &timeout);
}

/// One request log line: `<method> <route> <status> <n>ms`, where the
/// method is a standard one or `-`, and the route a route template or `-`.
fn assert_log_line(line: &str) {
    let parts: Vec<&str> = line.split(' ').collect();
    assert_eq!(parts.len(), 4, "{line:?}");
    assert!(
        [
            "GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "CONNECT", "TRACE", "-"
        ]
        .contains(&parts[0]),
        "{line:?}"
    );
    assert!(
        parts[1] == "-" || ROUTES.iter().any(|r| r.path == parts[1]),
        "{line:?}"
    );
    assert!(
        parts[2].len() == 3 && parts[2].chars().all(|c| c.is_ascii_digit()),
        "{line:?}"
    );
    let ms = parts[3].strip_suffix("ms").expect("time in ms");
    assert!(
        !ms.is_empty() && ms.chars().all(|c| c.is_ascii_digit()),
        "{line:?}"
    );
}

fn assert_log_has_no_content(log: &str, fake: &Fake) {
    for secret in [
        "thr_syn",
        "prj_syn",
        "Synthetic",
        "limit=",
        "detail=",
        "nowhere",
        "evil",
        "other.example",
        "Origin",
        "synthetic-model",
        "drip",
    ] {
        assert!(!log.contains(secret), "the log contains {secret}:\n{log}");
    }
    assert!(!log.contains(&fake.dir.display().to_string()));
}

#[test]
fn the_log_holds_method_route_status_and_time_only() {
    let fake = Fake::new("web_a7_log", "normal");
    let s = serve(&fake);
    let host = s.host();
    for target in [
        "/api/v1/inventory?detail=2&limit=3",
        "/api/v1/sessions/thr_syn_0002",
        "/api/v1/sessions/thr_syn_9999",
        "/api/v1/health",
        "/nowhere/thr_syn_0003",
        "/api/v1/inventory?limit=thr_syn_0004",
    ] {
        s.get(target);
    }
    s.request("POST", "/api/v1/sessions/thr_syn_0005");
    s.request("BREW", "/api/v1/inventory");
    s.send(b"GET /api/v1/health HTTP/1.1\r\nHost: evil.example.invalid\r\n\r\n");
    s.send(format!("GET /api/v1/health HTTP/1.1\r\nHost: {host}\r\nOrigin: https://other.example.invalid\r\n\r\n").as_bytes());
    s.send(b"garbage thr_syn_0006\r\n\r\n");
    // A line is logged after its response is sent and the connection
    // drained, so it can trail the client by a moment.
    let until = std::time::Instant::now() + Duration::from_secs(10);
    while s.log.lock().unwrap().len() < 11 && std::time::Instant::now() < until {
        thread::sleep(Duration::from_millis(10));
    }
    let log = s.log_text();
    let lines: Vec<&str> = log.lines().collect();
    assert_eq!(lines.len(), 11, "one line per request:\n{log}");
    for line in &lines {
        assert_log_line(line);
    }
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("GET /api/v1/sessions/{id} 404 ")),
        "{log}"
    );
    assert_log_has_no_content(&log, &fake);
}

#[test]
fn the_binary_logs_no_content_and_its_errors_name_no_path() {
    let program = http::server_binary();
    let fake = Fake::new("web_a7_binary", "normal");
    // No room-cli on PATH: the production wiring's error must name no path.
    let empty = fake.dir.join("empty-path");
    std::fs::create_dir_all(&empty).unwrap();
    let mut child = Command::new(&program)
        .args(["--port", "0"])
        .env("PATH", &empty)
        .current_dir(&fake.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start openwiz-server");
    let mut stderr = BufReader::new(child.stderr.take().unwrap());
    let mut first = String::new();
    stderr.read_line(&mut first).unwrap();
    let addr: Option<SocketAddr> = first
        .trim()
        .strip_prefix("openwiz-server: listening on http://")
        .and_then(|rest| rest.split(' ').next())
        .and_then(|a| a.parse().ok());
    let result = std::panic::catch_unwind(|| {
        let addr = addr.unwrap_or_else(|| panic!("unexpected first line {first:?}"));
        // An unreadable version is never a gate; the run goes on to
        // the session list, which fails too.
        let r = http::get(addr, "/api/v1/inventory?detail=2");
        assert_eq!(r.status, 502, "{}", r.text());
        let (kind, message) = r.error();
        assert_eq!(kind, "read_failed");
        assert!(message.starts_with("thread list "), "{message}");
        assert!(!message.contains('/'), "a path in {message}");
        assert_eq!(http::get(addr, "/api/v1/sessions/thr_syn_0001").status, 502);
        assert_eq!(http::get(addr, "/api/v1/health").status, 200);
    });
    // The three log lines can trail the responses by a moment: wait for
    // them before stopping the server.
    let mut rest = String::new();
    if result.is_ok() {
        for _ in 0..3 {
            let _ = stderr.read_line(&mut rest);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    let _ = stderr.read_to_string(&mut rest);
    let mut stdout = String::new();
    let _ = child.stdout.take().unwrap().read_to_string(&mut stdout);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
    assert!(stdout.is_empty(), "stdout: {stdout}");
    let lines: Vec<&str> = rest.lines().collect();
    assert_eq!(lines.len(), 3, "{rest}");
    for line in lines {
        let entry = line
            .strip_prefix("openwiz-server: ")
            .unwrap_or_else(|| panic!("{line:?}"));
        assert_log_line(entry);
    }
    assert_log_has_no_content(&rest, &fake);
}
