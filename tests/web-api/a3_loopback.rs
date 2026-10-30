//! A3 (fixture part): the server listens on loopback only, and refuses any
//! `Host` other than loopback or the one configured public host name (a
//! DNS-rebinding defence). The live LAN check is a procedure, not a test.

use std::io::{BufRead, BufReader};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::process::{Command, Stdio};
use std::time::Duration;

use openwiz_server::{ALLOWED_HOST_ENV, Config, DEFAULT_PORT, valid_host_name};

use crate::fake::Fake;
use crate::http::{self, non_loopback_address, serve, serve_with};

const SERVER_SRC: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../crates/server/src");

fn refused_from(ip: IpAddr, port: u16) -> bool {
    TcpStream::connect_timeout(&SocketAddr::new(ip, port), Duration::from_secs(2)).is_err()
}

#[test]
fn the_listener_is_bound_to_loopback_only() {
    let listener = openwiz_server::bind(0).expect("bind");
    let addr = listener.local_addr().expect("local address");
    assert_eq!(
        addr.ip(),
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        "the server must bind 127.0.0.1 only, not {addr}"
    );
    match non_loopback_address() {
        Some(ip) => assert!(
            refused_from(ip, addr.port()),
            "a connection to {ip}:{} (not loopback) was accepted",
            addr.port()
        ),
        None => eprintln!("no non-loopback address here; the LAN connect is not exercised"),
    }
}

#[test]
fn the_binary_listens_on_loopback_only_and_reads_nothing_at_start() {
    let program = http::server_binary();
    let fake = Fake::new("web_a3_binary", "normal");
    let mut child = Command::new(&program)
        .args(["--port", "0"])
        .env("PATH", &fake.dir)
        .env_remove(ALLOWED_HOST_ENV)
        .current_dir(&fake.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start openwiz-server");
    let mut stderr = BufReader::new(child.stderr.take().expect("stderr"));
    let mut first = String::new();
    stderr.read_line(&mut first).expect("read the first line");
    let result = std::panic::catch_unwind(|| {
        let addr: SocketAddr = first
            .trim()
            .strip_prefix("openwiz-server: listening on http://")
            .and_then(|rest| rest.split(' ').next())
            .unwrap_or_else(|| panic!("unexpected first line {first:?}"))
            .parse()
            .expect("a socket address");
        assert_eq!(addr.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST), "{first}");
        let health = http::get(addr, "/api/v1/health");
        assert_eq!(health.status, 200, "{}", health.text());
        if let Some(ip) = non_loopback_address() {
            assert!(refused_from(ip, addr.port()), "reachable at {ip}");
        }
        assert!(fake.invocations().is_empty(), "{:?}", fake.invocations());
    });
    let _ = child.kill();
    let _ = child.wait();
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

#[test]
fn the_binary_has_no_way_to_choose_the_bind_address() {
    let program = http::server_binary();
    for args in [
        &["--bind", "0.0.0.0"][..],
        &["--host", "0.0.0.0"][..],
        &["--listen", "0.0.0.0:7370"][..],
        &["--address", "::"][..],
        &["--port", "70000"][..],
        &["--port", "x"][..],
        &["--allowed-host", "not a host"][..],
        &["--allowed-host", "openwiz.example.invalid:443"][..],
    ] {
        let out = Command::new(&program)
            .args(args)
            .env_remove(ALLOWED_HOST_ENV)
            .output()
            .expect("run openwiz-server");
        assert_eq!(out.status.code(), Some(2), "{args:?} must be a usage error");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.starts_with("openwiz-server: "), "{args:?}: {err}");
    }
    let help = Command::new(&program)
        .arg("--help")
        .output()
        .expect("run --help");
    assert_eq!(help.status.code(), Some(0));
    let text = String::from_utf8_lossy(&help.stdout);
    assert!(text.contains("127.0.0.1"), "{text}");
    assert!(text.contains(&DEFAULT_PORT.to_string()), "{text}");
    assert!(text.contains(ALLOWED_HOST_ENV), "{text}");
}

#[test]
fn the_source_binds_only_one_loopback_address() {
    let mut text = String::new();
    for entry in std::fs::read_dir(SERVER_SRC).expect("server sources") {
        let path = entry.expect("entry").path();
        text.push_str(&std::fs::read_to_string(&path).expect("source"));
    }
    assert_eq!(
        text.matches("TcpListener::bind(").count(),
        1,
        "exactly one bind call"
    );
    assert!(text.contains("Ipv4Addr::LOCALHOST"), "binds 127.0.0.1");
    for wildcard in ["0.0.0.0", "UNSPECIFIED", "\"::\"", "Ipv6Addr::"] {
        assert!(!text.contains(wildcard), "the sources mention {wildcard}");
    }
}

#[test]
fn foreign_host_headers_are_refused() {
    let fake = Fake::new("web_a3_host", "normal");
    let s = serve(&fake);
    let port = s.addr.port();
    let with_host = |host: &str| {
        s.send(format!("GET /api/v1/health HTTP/1.1\r\nHost: {host}\r\n\r\n").as_bytes())
    };
    for host in [
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        format!("LocalHost:{port}"),
        format!("[::1]:{port}"),
    ] {
        assert_eq!(with_host(&host).status, 200, "{host}");
    }
    for host in [
        "openwiz.example.invalid".to_owned(),
        format!("openwiz.example.invalid:{port}"),
        "127.0.0.1".to_owned(),
        "localhost".to_owned(),
        format!("127.0.0.1:{}", port.wrapping_add(1)),
        format!("127.0.0.2:{port}"),
        format!("192.0.2.1:{port}"),
        format!("localhost.:{port}"),
        format!("localhost:{port}.example.invalid"),
    ] {
        let r = with_host(&host);
        assert_eq!(r.status, 421, "{host}: {}", r.text());
        assert_eq!(r.error().0, "misdirected", "{host}");
    }
    for raw in [
        "GET /api/v1/health HTTP/1.1\r\n\r\n".to_owned(),
        format!(
            "GET /api/v1/health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nHost: 127.0.0.1:{port}\r\n\r\n"
        ),
        format!("GET /api/v1/health HTTP/1.1\r\nHost: 127.0.0.1:{port}/x\r\n\r\n"),
        "GET /api/v1/health HTTP/1.1\r\nHost: \r\n\r\n".to_owned(),
    ] {
        let r = s.send(raw.as_bytes());
        assert_eq!(r.status, 400, "{raw:?}: {}", r.text());
    }
    assert!(fake.invocations().is_empty());
}

#[test]
fn a_foreign_host_gets_no_inventory() {
    let fake = Fake::new("web_a3_host_inventory", "normal");
    let s = serve(&fake);
    for target in ["/api/v1/inventory", "/api/v1/sessions/thr_syn_0001"] {
        let r = s.send(
            format!("GET {target} HTTP/1.1\r\nHost: rebound.example.invalid\r\n\r\n").as_bytes(),
        );
        assert_eq!(r.status, 421, "{target}");
        assert!(!r.text().contains("thr_syn"), "{}", r.text());
    }
    assert!(fake.invocations().is_empty(), "no read for a foreign host");
}

#[test]
fn the_configured_host_name_is_served() {
    let fake = Fake::new("web_a3_allowed_host", "normal");
    let s = serve_with(
        &fake,
        Config {
            allowed_host: Some("openwiz.example.invalid".to_owned()),
            ..Config::default()
        },
    );
    let with_host = |host: &str| {
        s.send(format!("GET /api/v1/health HTTP/1.1\r\nHost: {host}\r\n\r\n").as_bytes())
            .status
    };
    assert_eq!(with_host("openwiz.example.invalid"), 200);
    assert_eq!(with_host("OpenWiz.Example.Invalid"), 200);
    assert_eq!(with_host(&s.host()), 200, "loopback still works");
    assert_eq!(with_host("openwiz.example.invalid:443"), 421);
    assert_eq!(with_host("other.example.invalid"), 421);
    assert_eq!(with_host("wiz.example.invalid"), 421);
}

#[test]
fn only_dns_names_can_be_configured() {
    for good in ["openwiz.example.invalid", "a", "x-1.example.invalid", "A.B"] {
        assert!(valid_host_name(good), "{good}");
    }
    let long = format!("{}.example.invalid", "a".repeat(64));
    let too_long = ["a"; 128].join(".");
    for bad in [
        "",
        " ",
        "openwiz.example.invalid:443",
        "openwiz.example.invalid/",
        "http://openwiz.example.invalid",
        "-a.example.invalid",
        "a-.example.invalid",
        "a..example.invalid",
        ".example.invalid",
        "example.invalid.",
        "a_b.example.invalid",
        "*.example.invalid",
        "127.0.0.1:80",
        "[::1]",
        "ex\u{e4}mple.invalid",
        &long,
        &too_long,
    ] {
        assert!(!valid_host_name(bad), "{bad:?}");
    }
}
