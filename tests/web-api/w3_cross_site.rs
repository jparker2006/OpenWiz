use openwiz_server::{Limits, SendHost};

use crate::http::Response;
use crate::msg::SendFake;
use crate::send_support::{
    HOST, assert_no_cors, body, good_headers, messages, post, raw, send_config, serve_config,
    serve_send, with, without,
};

fn refused(fake: &SendFake, what: &str, r: &Response, status: u16, kind: &str, op_id: &str) {
    assert_eq!(r.status, status, "{what}: {}", r.text());
    assert_eq!(r.error().0, kind, "{what}");
    assert_no_cors(what, r);
    assert_eq!(fake.tell_count(), 0, "{what}: a tell was made");
    assert!(
        fake.invocations().is_empty(),
        "{what}: {:?}",
        fake.invocations()
    );
    assert!(
        !fake.record_path(op_id).exists(),
        "{what}: a record was claimed"
    );
}

#[test]
fn each_cross_site_vector_is_refused_with_no_call() {
    let fake = SendFake::new("web_w3_vectors", "send");
    let s = serve_send(&fake);
    let good = good_headers();
    type Case<'a> = (&'a str, Vec<(String, String)>, u16, &'a str);
    let cases: Vec<Case> = vec![
        (
            "no Origin",
            without(&good, "Origin"),
            403,
            "forbidden_origin",
        ),
        (
            "a foreign Origin",
            with(&good, "Origin", "https://attacker.example"),
            403,
            "forbidden_origin",
        ),
        (
            "a null Origin",
            with(&good, "Origin", "null"),
            403,
            "forbidden_origin",
        ),
        (
            "an http Origin",
            with(&good, "Origin", "http://openwiz.example"),
            403,
            "forbidden_origin",
        ),
        (
            "an Origin with a port",
            with(&good, "Origin", "https://openwiz.example:443"),
            403,
            "forbidden_origin",
        ),
        (
            "an Origin with a path",
            with(&good, "Origin", "https://openwiz.example/"),
            403,
            "forbidden_origin",
        ),
        (
            "a sibling subdomain Origin",
            with(&good, "Origin", "https://evil.openwiz.example"),
            403,
            "forbidden_origin",
        ),
        (
            "two Origins",
            {
                let mut h = good.clone();
                h.push(("Origin".to_owned(), "https://attacker.example".to_owned()));
                h
            },
            403,
            "forbidden_origin",
        ),
        (
            "Sec-Fetch-Site: cross-site",
            with(&good, "Sec-Fetch-Site", "cross-site"),
            403,
            "forbidden_origin",
        ),
        (
            "Sec-Fetch-Site: same-site",
            with(&good, "Sec-Fetch-Site", "same-site"),
            403,
            "forbidden_origin",
        ),
        (
            "Sec-Fetch-Site: none",
            with(&good, "Sec-Fetch-Site", "none"),
            403,
            "forbidden_origin",
        ),
        (
            "no Sec-Fetch-Site",
            without(&good, "Sec-Fetch-Site"),
            403,
            "forbidden_origin",
        ),
        (
            "no custom header",
            without(&good, "X-OpenWiz-Send"),
            403,
            "missing_custom_header",
        ),
        (
            "a custom header of 0",
            with(&good, "X-OpenWiz-Send", "0"),
            403,
            "missing_custom_header",
        ),
        (
            "text/plain",
            with(&good, "Content-Type", "text/plain"),
            415,
            "unsupported_media_type",
        ),
        (
            "a form's content type",
            with(&good, "Content-Type", "application/x-www-form-urlencoded"),
            415,
            "unsupported_media_type",
        ),
        (
            "multipart",
            with(&good, "Content-Type", "multipart/form-data; boundary=x"),
            415,
            "unsupported_media_type",
        ),
        (
            "another charset",
            with(&good, "Content-Type", "application/json; charset=latin1"),
            415,
            "unsupported_media_type",
        ),
        (
            "no content type",
            without(&good, "Content-Type"),
            415,
            "unsupported_media_type",
        ),
        (
            "a cross-site form post",
            vec![
                ("Host".to_owned(), HOST.to_owned()),
                ("Origin".to_owned(), "https://attacker.example".to_owned()),
                ("Sec-Fetch-Site".to_owned(), "cross-site".to_owned()),
                (
                    "Content-Type".to_owned(),
                    "application/x-www-form-urlencoded".to_owned(),
                ),
            ],
            403,
            "forbidden_origin",
        ),
        (
            "a rebinding Host",
            with(&good, "Host", "attacker.example"),
            421,
            "misdirected",
        ),
        (
            "a loopback Host",
            with(&good, "Host", &s.host()),
            421,
            "misdirected",
        ),
        (
            "a localhost Host",
            with(&good, "Host", &format!("localhost:{}", s.addr.port())),
            421,
            "misdirected",
        ),
    ];
    let payload = body("w3-op-1", "queue", "Synthetic W3 text").to_string();
    for (what, headers, status, kind) in cases {
        let r = post(&s, "thr_syn_0001", &headers, payload.as_bytes());
        refused(&fake, what, &r, status, kind, "w3-op-1");
    }
}

#[test]
fn a_json_content_type_with_utf_8_is_accepted() {
    for (n, value) in [
        "application/json",
        "application/json; charset=utf-8",
        "Application/JSON;charset=UTF-8",
    ]
    .iter()
    .enumerate()
    {
        let fake = SendFake::new(&format!("web_w3_type_{n}"), "send");
        let s = serve_send(&fake);
        let headers = with(&good_headers(), "Content-Type", value);
        let payload = body("w3-op-2", "queue", "Synthetic W3 text").to_string();
        let r = post(&s, "thr_syn_0001", &headers, payload.as_bytes());
        assert_eq!(r.status, 200, "{value}: {}", r.text());
        assert_no_cors(value, &r);
        assert_eq!(fake.tell_count(), 1, "{value}");
    }
}

#[test]
fn without_a_configured_host_name_nothing_can_send() {
    let fake = SendFake::new("web_w3_no_host", "send");
    let mut config = send_config(&fake, Limits::default());
    config.allowed_host = None;
    config.sending.as_mut().expect("sending").host = None;
    let s = serve_config(&fake, config);
    let payload = body("w3-op-3", "queue", "Synthetic W3 text").to_string();
    for host in [s.host(), HOST.to_owned()] {
        let r = post(
            &s,
            "thr_syn_0001",
            &with(&good_headers(), "Host", &host),
            payload.as_bytes(),
        );
        refused(&fake, &host, &r, 421, "misdirected", "w3-op-3");
    }
}

#[test]
fn a_preflight_gets_no_cors_grant() {
    let fake = SendFake::new("web_w3_preflight", "send");
    let s = serve_send(&fake);
    let headers = vec![
        ("Host".to_owned(), HOST.to_owned()),
        ("Origin".to_owned(), "https://attacker.example".to_owned()),
        (
            "Access-Control-Request-Method".to_owned(),
            "POST".to_owned(),
        ),
        (
            "Access-Control-Request-Headers".to_owned(),
            "content-type, x-openwiz-send".to_owned(),
        ),
    ];
    let r = s.send(&raw(
        "OPTIONS",
        &messages("thr_syn_0001"),
        &headers,
        b"",
        false,
    ));
    assert_eq!(r.status, 405, "{}", r.status_line);
    assert_no_cors("OPTIONS", &r);
    assert_eq!(fake.tell_count(), 0);
    // And the accepted send carries no CORS header either.
    let r = post(
        &s,
        "thr_syn_0001",
        &good_headers(),
        body("w3-op-4", "queue", "Synthetic W3 text")
            .to_string()
            .as_bytes(),
    );
    assert_eq!(r.status, 200, "{}", r.text());
    assert_no_cors("the send", &r);
}

#[test]
fn a_send_host_can_never_be_loopback_or_an_ip_address() {
    for bad in [
        "localhost",
        "LOCALHOST",
        "LocalHost",
        "a.localhost",
        "openwiz.localhost",
        "localhost.localdomain",
        "ip6-localhost",
        "ip6-loopback",
        "127.0.0.1",
        "127.1",
        "127.0.0.1.",
        "2130706433",
        "0.0.0.0",
        "10.0.0.1",
        "203.0.113.7",
        "1.2.3.4",
        "0x7f.0.0.1",
        "::1",
        "[::1]",
        "",
        "openwiz.example:443",
    ] {
        assert!(
            SendHost::new(bad).is_err(),
            "{bad:?} was accepted as a send host"
        );
    }
    for good in [
        "openwiz.example",
        "my.openwiz.example",
        "a",
        "x-1.example.invalid",
    ] {
        assert_eq!(SendHost::new(good).expect(good).as_str(), good);
    }
}

#[test]
fn the_binary_refuses_a_loopback_or_ip_send_host_before_any_access() {
    let fake = SendFake::new("web_w3_loopback_binary", "send");
    for host in ["localhost", "127.0.0.1", "a.localhost", "203.0.113.7"] {
        let mut child = std::process::Command::new(crate::http::server_binary())
            .args(["--port", "0", "--allowed-host", host])
            .env("PATH", fake.path())
            .env("OPENWIZ_STATE_DIR", &fake.state)
            .current_dir(&fake.dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("run openwiz-server");
        // Never wait forever: a server that starts fails the proof.
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while child.try_wait().expect("the server's state").is_none() {
            if std::time::Instant::now() >= until {
                let _ = child.kill();
                let _ = child.wait();
                panic!("{host}: the server started with a loopback or IP send host");
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let out = child.wait_with_output().expect("its output");
        assert_eq!(out.status.code(), Some(2), "{host}: {out:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!err.contains("listening"), "{host}: {err}");
        assert!(
            !err.contains(host) || host == "a",
            "{host}: the value is echoed: {err}"
        );
    }
    assert!(fake.invocations().is_empty(), "{:?}", fake.invocations());
    assert!(!fake.state.exists(), "the send record was touched");
}
