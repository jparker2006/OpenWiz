//! A5: requests are bounded. Queries are parsed strictly against the core's
//! caps; bodies, heads, targets and connections are capped; identical
//! concurrent requests share one core run; core runs are capped, with a
//! bounded wait and then `503`; slow clients time out; every request has a
//! deadline above the core's 30-second budget.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use openwiz_core::limits::{MAX_LIMIT, RUN_BUDGET};
use openwiz_server::{
    Config, HEAD_TIMEOUT, Limits, MAX_CONNECTIONS, MAX_CORE_RUNS, MAX_HEAD_BYTES, MAX_HEADER_LINES,
    MAX_TARGET_BYTES, REQUEST_DEADLINE, RETRY_AFTER_SECS, SLOT_WAIT, WRITE_TIMEOUT,
};

use crate::fake::Fake;
use crate::http::{self, Gated, read_response, serve, serve_source, serve_with};

fn config(limits: Limits) -> Config {
    Config {
        allowed_host: None,
        web_root: None,
        limits,
    }
}

#[test]
fn the_limits_are_the_documented_values() {
    assert_eq!(MAX_CONNECTIONS, 32);
    assert_eq!(MAX_CORE_RUNS, 2);
    assert_eq!(SLOT_WAIT, Duration::from_secs(5));
    assert_eq!(RETRY_AFTER_SECS, 5);
    assert_eq!(REQUEST_DEADLINE, Duration::from_secs(40));
    assert_eq!(HEAD_TIMEOUT, Duration::from_secs(10));
    assert_eq!(WRITE_TIMEOUT, Duration::from_secs(10));
    assert_eq!(MAX_TARGET_BYTES, 1024);
    assert_eq!(MAX_HEAD_BYTES, 16 * 1024);
    assert_eq!(MAX_HEADER_LINES, 64);
    assert!(
        REQUEST_DEADLINE > RUN_BUDGET + SLOT_WAIT,
        "the request deadline must sit above the core's run budget and the slot wait"
    );
    assert_eq!(Limits::default().request_deadline, REQUEST_DEADLINE);
    assert_eq!(Limits::default().max_core_runs, MAX_CORE_RUNS);
}

#[test]
fn malformed_and_out_of_range_queries_are_rejected_without_a_read() {
    let fake = Fake::new("web_a5_queries", "normal");
    let s = serve(&fake);
    for query in [
        "limit=",
        "limit=-1",
        "limit=+5",
        "limit=1.5",
        "limit=0x10",
        "limit=1e2",
        "limit=5%20",
        "limit=%35",
        "limit=99999999999999999999999",
        "offset=",
        "offset=-1",
        "offset=abc",
        "offset=18446744073709551616",
        "detail=",
        "detail=0",
        "detail=3",
        "detail=long",
        "hidden=",
        "hidden=yes",
        "hidden=true",
        "hidden=2",
        "archived=on",
        "archived=-1",
        "limit=1&limit=2",
        "detail=1&detail=1",
        "hidden=1&hidden=0",
        "unknown=1",
        "Limit=5",
        "limit",
        "limit=5&",
        "&",
        "&&",
        "=5",
        "a=b=c",
        "%6Cimit=5",
        "limit=5;offset=1",
    ] {
        let r = s.get(&format!("/api/v1/inventory?{query}"));
        assert_eq!(r.status, 400, "{query}: {}", r.text());
        assert_eq!(r.error().0, "invalid_request", "{query}");
    }
    assert!(fake.invocations().is_empty(), "{:?}", fake.invocations());
}

#[test]
fn the_cores_limit_cap_is_applied_without_a_read() {
    let fake = Fake::new("web_a5_limit_cap", "normal");
    let s = serve(&fake);
    let r = s.get(&format!("/api/v1/inventory?limit={}", MAX_LIMIT + 1));
    assert_eq!(r.status, 400, "{}", r.text());
    assert_eq!(
        r.error(),
        (
            "invalid_request".to_owned(),
            format!("--limit {} is above the cap of {MAX_LIMIT}", MAX_LIMIT + 1)
        ),
        "the core's own words"
    );
    assert!(fake.invocations().is_empty());
    for query in [
        format!("limit={MAX_LIMIT}"),
        "limit=0".to_owned(),
        "offset=0".to_owned(),
        "offset=1000000".to_owned(),
        format!("limit={}1", "0".repeat(26)),
        "detail=1&hidden=0&archived=0&limit=100&offset=0".to_owned(),
        "".to_owned(),
    ] {
        let r = s.get(&format!("/api/v1/inventory?{query}"));
        assert_eq!(r.status, 200, "{query}: {}", r.text());
    }
}

#[test]
fn health_and_show_take_no_query() {
    let fake = Fake::new("web_a5_no_query", "normal");
    let s = serve(&fake);
    for target in [
        "/api/v1/health?x=1",
        "/api/v1/health?",
        "/api/v1/sessions/thr_syn_0001?detail=2",
        "/api/v1/sessions/thr_syn_0001?",
    ] {
        let r = s.get(target);
        assert_eq!(r.status, 400, "{target}: {}", r.text());
        assert_eq!(r.error().0, "invalid_request");
    }
    assert!(fake.invocations().is_empty());
}

#[test]
fn ids_the_adapter_refuses_are_rejected_by_the_core() {
    let fake = Fake::new("web_a5_ids", "normal");
    let s = serve(&fake);
    for id in [
        "-x".to_owned(),
        "thr%20syn".to_owned(),
        "thr%2Fsyn".to_owned(),
        "thr.syn".to_owned(),
        "a".repeat(129),
    ] {
        http::clear_invocations(&fake);
        let r = s.get(&format!("/api/v1/sessions/{id}"));
        assert_eq!(r.status, 400, "{id}: {}", r.text());
        let (kind, message) = r.error();
        assert_eq!(kind, "invalid_request");
        assert!(
            message.contains("cannot be passed to room-cli"),
            "{message}"
        );
        // As the CLI: the version is read first, then nothing.
        assert_eq!(fake.invocations(), [["--version"]], "{id}");
    }
}

#[test]
fn request_bodies_are_rejected() {
    let fake = Fake::new("web_a5_bodies", "normal");
    let s = serve(&fake);
    let host = s.host();
    for raw in [
        format!("GET /api/v1/inventory HTTP/1.1\r\nHost: {host}\r\nContent-Length: 4\r\n\r\nabcd"),
        format!(
            "GET /api/v1/inventory HTTP/1.1\r\nHost: {host}\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nabcd\r\n0\r\n\r\n"
        ),
        format!("GET /api/v1/inventory HTTP/1.1\r\nHost: {host}\r\nContent-Length: x\r\n\r\n"),
        format!(
            "GET /api/v1/inventory HTTP/1.1\r\nHost: {host}\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n"
        ),
    ] {
        let r = s.send(raw.as_bytes());
        assert_eq!(r.status, 400, "{raw:?}: {}", r.text());
        assert_eq!(r.error().0, "bad_request");
    }
    assert!(fake.invocations().is_empty());
    let r = s.send(
        format!("GET /api/v1/health HTTP/1.1\r\nHost: {host}\r\nContent-Length: 0\r\n\r\n")
            .as_bytes(),
    );
    assert_eq!(r.status, 200, "an empty body is no body");
}

#[test]
fn targets_and_heads_are_capped() {
    let fake = Fake::new("web_a5_heads", "normal");
    let s = serve(&fake);
    let host = s.host();
    let at_cap = format!("/{}", "a".repeat(MAX_TARGET_BYTES - 1));
    assert_eq!(s.get(&at_cap).status, 404, "a target at the cap is read");
    let over = format!("/{}", "a".repeat(MAX_TARGET_BYTES));
    let r = s.get(&over);
    assert_eq!(r.status, 414, "{}", r.text());
    assert_eq!(r.error().0, "target_too_long");

    let many: String = (0..=MAX_HEADER_LINES)
        .map(|i| format!("X-Synthetic-{i}: 1\r\n"))
        .collect();
    let r = s.send(format!("GET /api/v1/health HTTP/1.1\r\nHost: {host}\r\n{many}\r\n").as_bytes());
    assert_eq!(r.status, 431, "{}", r.text());
    assert_eq!(r.error().0, "head_too_large");

    let big = "b".repeat(MAX_HEAD_BYTES);
    let r = s.send(
        format!("GET /api/v1/health HTTP/1.1\r\nHost: {host}\r\nX-Synthetic: {big}\r\n\r\n")
            .as_bytes(),
    );
    assert_eq!(r.status, 431, "{}", r.text());

    // Just under the caps is served.
    let fine: String = (0..MAX_HEADER_LINES - 1)
        .map(|i| format!("X-Synthetic-{i}: 1\r\n"))
        .collect();
    let r = s.send(format!("GET /api/v1/health HTTP/1.1\r\nHost: {host}\r\n{fine}\r\n").as_bytes());
    assert_eq!(r.status, 200, "{}", r.text());
    assert!(fake.invocations().is_empty());
}

#[test]
fn malformed_requests_are_rejected() {
    let fake = Fake::new("web_a5_malformed", "normal");
    let s = serve(&fake);
    let host = s.host();
    for (raw, status) in [
        ("GET /api/v1/health\r\n\r\n".to_owned(), 400),
        (
            format!("GET  /api/v1/health HTTP/1.1\r\nHost: {host}\r\n\r\n"),
            400,
        ),
        (
            format!("GET /api/v1/health  HTTP/1.1\r\nHost: {host}\r\n\r\n"),
            400,
        ),
        (
            format!("GET /api/v1/health HTTP/1.1 \r\nHost: {host}\r\n\r\n"),
            400,
        ),
        (
            format!("GET http://{host}/api/v1/health HTTP/1.1\r\nHost: {host}\r\n\r\n"),
            400,
        ),
        (format!("GET * HTTP/1.1\r\nHost: {host}\r\n\r\n"), 400),
        (
            format!("GET api/v1/health HTTP/1.1\r\nHost: {host}\r\n\r\n"),
            400,
        ),
        (
            format!("GET /api/v1/health#x HTTP/1.1\r\nHost: {host}\r\n\r\n"),
            400,
        ),
        (
            format!("GET /api/v1/he\u{e4}lth HTTP/1.1\r\nHost: {host}\r\n\r\n"),
            400,
        ),
        (
            format!("GET /api/v1/health HTTP/2.0\r\nHost: {host}\r\n\r\n"),
            505,
        ),
        (
            format!("GET /api/v1/health HTTP/0.9\r\nHost: {host}\r\n\r\n"),
            505,
        ),
        (
            format!("GET /api/v1/health http/1.1\r\nHost: {host}\r\n\r\n"),
            400,
        ),
        (
            format!("GET /api/v1/health HTTP/1.1\r\nHost {host}\r\n\r\n"),
            400,
        ),
        (
            format!("GET /api/v1/health HTTP/1.1\r\nHost : {host}\r\n\r\n"),
            400,
        ),
        (
            format!("GET /api/v1/health HTTP/1.1\r\nHost: {host}\r\n folded\r\n\r\n"),
            400,
        ),
        (
            format!("GET /api/v1/health HTTP/1.1\nHost: {host}\n\n"),
            400,
        ),
        (
            format!("GET /api/v1/health HTTP/1.1\r\nHost: {host}\r\nX: a\u{0}b\r\n\r\n"),
            400,
        ),
        (
            format!("\r\nGET /api/v1/health HTTP/1.1\r\nHost: {host}\r\n\r\n"),
            400,
        ),
        (
            format!("G\u{0}T /api/v1/health HTTP/1.1\r\nHost: {host}\r\n\r\n"),
            400,
        ),
    ] {
        let r = s.send(raw.as_bytes());
        assert_eq!(r.status, status, "{raw:?}: {}", r.text());
    }
    // HTTP/1.0 is served, with a Host.
    let r = s.send(format!("GET /api/v1/health HTTP/1.0\r\nHost: {host}\r\n\r\n").as_bytes());
    assert_eq!(r.status, 200);
    assert!(fake.invocations().is_empty());
}

#[test]
fn a_slow_client_times_out_and_does_not_block_others() {
    let fake = Fake::new("web_a5_slow", "normal");
    let s = serve_with(
        &fake,
        config(Limits {
            head_timeout: Duration::from_millis(800),
            ..Limits::default()
        }),
    );
    // Half a request, then nothing.
    let started = Instant::now();
    let mut idle = TcpStream::connect(s.addr).unwrap();
    idle.set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    idle.write_all(b"GET /api/v1/he").unwrap();
    // Another client is served meanwhile.
    assert_eq!(s.get("/api/v1/health").status, 200);
    let r = read_response(&mut idle);
    let waited = started.elapsed();
    assert_eq!(r.status, 408, "{}", r.text());
    assert_eq!(r.error().0, "request_timeout");
    assert!(
        waited >= Duration::from_millis(700) && waited < Duration::from_secs(3),
        "{waited:?}"
    );

    // One byte at a time: the deadline is for the whole head.
    let started = Instant::now();
    let mut drip = TcpStream::connect(s.addr).unwrap();
    drip.set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut writer = drip.try_clone().unwrap();
    let dripper = thread::spawn(move || {
        for byte in b"GET /api/v1/health HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Synthetic: drip\r\n" {
            if writer.write_all(&[*byte]).is_err() {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
    });
    let r = read_response(&mut drip);
    let waited = started.elapsed();
    assert_eq!(r.status, 408, "{}", r.text());
    assert!(waited < Duration::from_secs(3), "{waited:?}");
    let _ = dripper.join();
    s.wait_for("the connections to close", |st| st.connections == 0);
}

#[test]
fn connections_are_capped() {
    let fake = Fake::new("web_a5_connections", "normal");
    let s = serve_with(
        &fake,
        config(Limits {
            max_connections: 3,
            head_timeout: Duration::from_secs(5),
            ..Limits::default()
        }),
    );
    let idle: Vec<TcpStream> = (0..3)
        .map(|_| TcpStream::connect(s.addr).unwrap())
        .collect();
    s.wait_for("three connections", |st| st.connections == 3);
    let started = Instant::now();
    let r = s.get("/api/v1/health");
    assert_eq!(r.status, 503, "{}", r.text());
    assert_eq!(r.error().0, "busy");
    assert_eq!(r.header("Retry-After"), Some("5"));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "answered at once"
    );
    drop(idle);
    s.wait_for("the connections to close", |st| st.connections == 0);
    assert_eq!(s.get("/api/v1/health").status, 200);
    assert!(fake.invocations().is_empty());
}

#[test]
fn identical_concurrent_requests_share_one_core_run() {
    let fake = Fake::new("web_a5_single_flight", "normal");
    let gated = Arc::new(Gated::new(Arc::new(fake.source())));
    let s = serve_source(gated.clone(), Config::default());
    let addr = s.addr;
    let clients: Vec<_> = (0..8)
        .map(|_| thread::spawn(move || http::get(addr, "/api/v1/inventory?detail=2")))
        .collect();
    s.wait_for("eight requests on one run", |st| {
        st.attached == 8 && st.core_runs == 1
    });
    assert_eq!(gated.runs(), 1);
    gated.open();
    let responses: Vec<_> = clients.into_iter().map(|c| c.join().unwrap()).collect();
    for r in &responses {
        assert_eq!(r.status, 200, "{}", r.text());
        assert_eq!(
            r.body, responses[0].body,
            "every request gets the same result"
        );
    }
    assert_eq!(gated.runs(), 1, "one core run for eight identical requests");
    let lists = fake
        .invocations()
        .iter()
        .filter(|i| i.starts_with(&["thread".to_owned(), "list".to_owned()]))
        .count();
    assert_eq!(lists, 1, "{:?}", fake.invocations());

    // Nothing is cached: the next request is a new run.
    assert_eq!(s.get("/api/v1/inventory?detail=2").status, 200);
    assert_eq!(gated.runs(), 2);
}

#[test]
fn identical_show_requests_share_one_core_run() {
    let fake = Fake::new("web_a5_single_flight_show", "normal");
    let gated = Arc::new(Gated::new(Arc::new(fake.source())));
    let s = serve_source(gated.clone(), Config::default());
    let addr = s.addr;
    let clients: Vec<_> = (0..4)
        .map(|_| thread::spawn(move || http::get(addr, "/api/v1/sessions/thr_syn_0002")))
        .collect();
    s.wait_for("four requests on one run", |st| {
        st.attached == 4 && st.core_runs == 1
    });
    gated.open();
    for c in clients {
        assert_eq!(c.join().unwrap().status, 200);
    }
    assert_eq!(gated.runs(), 1);
}

#[test]
fn core_runs_are_capped_and_the_overflow_gets_503_after_a_bounded_wait() {
    let fake = Fake::new("web_a5_run_cap", "normal");
    let gated = Arc::new(Gated::new(Arc::new(fake.source())));
    let s = serve_source(
        gated.clone(),
        config(Limits {
            slot_wait: Duration::from_millis(600),
            ..Limits::default()
        }),
    );
    let addr = s.addr;
    let first: Vec<_> = ["limit=1", "limit=2"]
        .into_iter()
        .map(|q| thread::spawn(move || http::get(addr, &format!("/api/v1/inventory?{q}"))))
        .collect();
    gated.wait_held(MAX_CORE_RUNS);
    let started = Instant::now();
    let r = s.get("/api/v1/inventory?limit=3");
    let waited = started.elapsed();
    assert_eq!(r.status, 503, "{}", r.text());
    assert_eq!(r.error().0, "busy");
    assert_eq!(r.header("Retry-After"), Some("5"));
    assert!(
        waited >= Duration::from_millis(500) && waited < Duration::from_secs(3),
        "a bounded wait, then 503: {waited:?}"
    );
    assert_eq!(gated.max_held(), MAX_CORE_RUNS, "never more than the cap");
    assert_eq!(
        gated.runs(),
        MAX_CORE_RUNS,
        "the refused request ran nothing"
    );
    gated.open();
    for c in first {
        assert_eq!(c.join().unwrap().status, 200);
    }
    s.wait_for("the runs to end", |st| st.core_runs == 0);
}

#[test]
fn a_waiting_request_runs_when_a_slot_frees() {
    let fake = Fake::new("web_a5_run_wait", "normal");
    let gated = Arc::new(Gated::new(Arc::new(fake.source())));
    let s = serve_source(
        gated.clone(),
        config(Limits {
            max_core_runs: 1,
            slot_wait: Duration::from_secs(10),
            ..Limits::default()
        }),
    );
    let addr = s.addr;
    let a = thread::spawn(move || http::get(addr, "/api/v1/inventory?limit=1"));
    gated.wait_held(1);
    let b = thread::spawn(move || http::get(addr, "/api/v1/sessions/thr_syn_0001"));
    thread::sleep(Duration::from_millis(200));
    assert_eq!(gated.runs(), 1, "the second request waits for the slot");
    gated.open();
    assert_eq!(a.join().unwrap().status, 200);
    assert_eq!(b.join().unwrap().status, 200);
    assert_eq!(gated.max_held(), 1);
}

#[test]
fn a_request_past_its_deadline_gets_504_and_the_run_keeps_its_slot() {
    let fake = Fake::new("web_a5_deadline", "normal");
    let gated = Arc::new(Gated::new(Arc::new(fake.source())));
    let s = serve_source(
        gated.clone(),
        config(Limits {
            request_deadline: Duration::from_millis(700),
            ..Limits::default()
        }),
    );
    let started = Instant::now();
    let r = s.get("/api/v1/inventory");
    let waited = started.elapsed();
    assert_eq!(r.status, 504, "{}", r.text());
    assert_eq!(r.error().0, "deadline");
    assert!(
        waited >= Duration::from_millis(600) && waited < Duration::from_secs(3),
        "{waited:?}"
    );
    // The core run is still going, and still counts against the cap.
    assert_eq!(s.server.stats().core_runs, 1);
    gated.open();
    s.wait_for("the run to end", |st| st.core_runs == 0);
}

/// P1: past both connection caps (the handlers and the equally capped set
/// that answers `503`), a connection is answered at once by the accept
/// loop with a bodyless `503`, never closed unanswered. Deterministic: a
/// refused client that keeps its connection open holds the only refusal
/// slot for the server's linger, so the next client is past both caps.
#[test]
fn past_both_connection_caps_a_client_still_gets_a_503() {
    let fake = Fake::new("web_a5_past_both_caps", "normal");
    let s = serve_with(
        &fake,
        config(Limits {
            max_connections: 1,
            head_timeout: Duration::from_secs(5),
            ..Limits::default()
        }),
    );
    let _idle = TcpStream::connect(s.addr).unwrap();
    s.wait_for("one connection", |st| st.connections == 1);
    // Refused, and kept open without reading: its refusal thread lingers.
    let mut held = TcpStream::connect(s.addr).unwrap();
    held.write_all(format!("GET /api/v1/health HTTP/1.1\r\nHost: {}\r\n\r\n", s.host()).as_bytes())
        .unwrap();
    for method in ["GET", "HEAD"] {
        let mut late = TcpStream::connect(s.addr).unwrap();
        late.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let _ = late.write_all(
            format!(
                "{method} /api/v1/health HTTP/1.1\r\nHost: {}\r\n\r\n",
                s.host()
            )
            .as_bytes(),
        );
        let mut bytes = Vec::new();
        let _ = late.read_to_end(&mut bytes);
        assert!(
            !bytes.is_empty(),
            "{method} past both caps: the connection was closed unanswered"
        );
        let r = http::parse_response(&bytes);
        assert_eq!(r.status, 503, "{method}");
        assert_eq!(r.header("Retry-After"), Some("5"), "{method}");
        assert_eq!(r.header("Cache-Control"), Some("no-store"), "{method}");
        assert_eq!(r.header("Connection"), Some("close"), "{method}");
        // The accept loop cannot wait to learn the method: no body and no
        // Content-Length, which suits GET and HEAD alike.
        assert!(r.body.is_empty(), "{method}: {} body bytes", r.body.len());
        assert_eq!(r.header("Content-Length"), None, "{method}");
    }
    drop(held);
    assert!(fake.invocations().is_empty());
}
