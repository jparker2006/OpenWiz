//! AW2: owner only, and no cross-site write, for every attachment
//! route, through the shared request checks. The
//! owner is the tunnel's to check: the server reads no Access
//! header or cookie. An upload or a removal must carry the configured host
//! name, the site's `Origin`, `Sec-Fetch-Site: same-origin` and
//! `X-OpenWiz-Send: 1`; an upload, exactly `application/octet-stream`,
//! never a form's type. **Each refusal stages nothing and removes nothing.**

use openwiz_server::Limits;

use crate::attach_support::{
    ATTACHMENTS, Free, State, attachment, ready_id, remove_headers, serve_attach, upload,
    upload_headers, upload_with,
};
use crate::send_support::{raw, with, without};

const FILE: &[u8] = b"Synthetic AW2 text\n";

/// One refused request: what it is, its headers, and the status and kind it gets.
type Refusal = (String, Vec<(String, String)>, u16, &'static str);

/// Every refused form of a header set: missing, wrong, repeated.
fn refusals(good: &[(String, String)], body_type: bool, port: u16) -> Vec<Refusal> {
    let mut out = vec![];
    let mut case = |what: &str, headers: Vec<(String, String)>, status, kind| {
        out.push((what.to_owned(), headers, status, kind));
    };
    // This server's own loopback name: served for reads, never for a write.
    for host in [
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        format!("[::1]:{port}"),
    ] {
        case(
            &format!("Host {host}"),
            with(good, "Host", &host),
            421,
            "misdirected",
        );
    }
    case(
        "another Host",
        with(good, "Host", "elsewhere.example"),
        421,
        "misdirected",
    );
    case(
        "no Origin",
        without(good, "Origin"),
        403,
        "forbidden_origin",
    );
    for origin in [
        "https://elsewhere.example",
        "http://openwiz.example",
        "https://openwiz.example:443",
        "https://openwiz.example/",
        "null",
        "https://sub.openwiz.example",
    ] {
        case(
            &format!("Origin {origin}"),
            with(good, "Origin", origin),
            403,
            "forbidden_origin",
        );
    }
    let mut twice = good.to_vec();
    twice.push(("Origin".to_owned(), "https://openwiz.example".to_owned()));
    case("Origin twice", twice, 403, "forbidden_origin");
    case(
        "no Sec-Fetch-Site",
        without(good, "Sec-Fetch-Site"),
        403,
        "forbidden_origin",
    );
    for site in ["cross-site", "same-site", "none", "Same-Origin"] {
        case(
            &format!("Sec-Fetch-Site {site}"),
            with(good, "Sec-Fetch-Site", site),
            403,
            "forbidden_origin",
        );
    }
    case(
        "no custom header",
        without(good, "X-OpenWiz-Send"),
        403,
        "missing_custom_header",
    );
    for v in ["0", "true", "", "11", "1, 1"] {
        case(
            &format!("X-OpenWiz-Send {v:?}"),
            with(good, "X-OpenWiz-Send", v),
            403,
            "missing_custom_header",
        );
    }
    if body_type {
        case(
            "no Content-Type",
            without(good, "Content-Type"),
            415,
            "unsupported_media_type",
        );
        for t in [
            "multipart/form-data; boundary=synthetic",
            "application/x-www-form-urlencoded",
            "text/plain",
            "application/json",
            "application/octet-stream; charset=utf-8",
            "application/octet-streamx",
            "image/png",
        ] {
            case(
                &format!("Content-Type {t}"),
                with(good, "Content-Type", t),
                415,
                "unsupported_media_type",
            );
        }
    }
    out
}

#[test]
fn every_cross_site_or_misdirected_upload_is_refused_and_stages_nothing() {
    let state = State::new("web_aw2_upload");
    let s = serve_attach(&state, Free::plenty(), Limits::default());
    for (what, headers, status, kind) in refusals(&upload_headers("aw2.txt"), true, s.addr.port()) {
        let r = upload_with(&s, &headers, FILE);
        assert_eq!(r.status, status, "{what}: {}", r.text());
        assert_eq!(r.error().0, kind, "{what}");
        crate::send_support::assert_no_cors(&what, &r);
        assert!(state.nothing_staged(), "{what}: {:?}", state.entries());
    }
    // A cross-site form's own shape: multipart, no custom header, its own
    // origin. Refused before the body is looked at.
    let form = vec![
        ("Host".to_owned(), "openwiz.example".to_owned()),
        ("Origin".to_owned(), "https://elsewhere.example".to_owned()),
        ("Sec-Fetch-Site".to_owned(), "cross-site".to_owned()),
        (
            "Content-Type".to_owned(),
            "multipart/form-data; boundary=synthetic".to_owned(),
        ),
    ];
    let body = b"--synthetic\r\nContent-Disposition: form-data; name=\"file\"; filename=\"aw2.txt\"\r\n\r\nSynthetic\r\n--synthetic--\r\n";
    let r = upload_with(&s, &form, body);
    assert_eq!(r.status, 403, "{}", r.text());
    assert!(state.nothing_staged());
    // And the good upload still works.
    ready_id(&upload(&s, "aw2.txt", FILE));
    assert_eq!(state.entries().len(), 1);
}

#[test]
fn every_cross_site_or_misdirected_removal_is_refused_and_removes_nothing() {
    let state = State::new("web_aw2_remove");
    let s = serve_attach(&state, Free::plenty(), Limits::default());
    let id = ready_id(&upload(&s, "aw2.txt", FILE));
    for (what, headers, status, kind) in refusals(&remove_headers(), false, s.addr.port()) {
        let r = s.send(&raw("DELETE", &attachment(&id), &headers, b"", false));
        assert_eq!(r.status, status, "{what}: {}", r.text());
        assert_eq!(r.error().0, kind, "{what}");
        assert_eq!(state.entries().len(), 1, "{what}: removed");
    }
    let r = crate::attach_support::remove(&s, &id);
    assert_eq!(r.status, 200, "{}", r.text());
    assert!(state.nothing_staged());
}

#[test]
fn without_an_allowed_host_there_is_no_upload_or_removal() {
    let state = State::new("web_aw2_no_host");
    let mut config =
        crate::attach_support::attach_config(&state.dir, Free::plenty(), Limits::default());
    config.attaching.as_mut().unwrap().host = None;
    let fake = crate::fake::Fake::new("web_aw2_no_host_source", "normal");
    let s = crate::attach_support::serve(
        config,
        std::sync::Arc::new(fake.source()),
        std::sync::Arc::new(crate::new_support::clock()),
    );
    let r = upload(&s, "aw2.txt", FILE);
    assert_eq!(r.status, 421, "{}", r.text());
    let r = crate::attach_support::remove(&s, "att-0123456789abcdef0123456789abcdef");
    assert_eq!(r.status, 421, "{}", r.text());
    assert!(state.nothing_staged());
}

#[test]
fn the_list_follows_the_usual_get_rules_and_never_needs_an_origin() {
    let state = State::new("web_aw2_list");
    let s = serve_attach(&state, Free::plenty(), Limits::default());
    ready_id(&upload(&s, "aw2.txt", FILE));
    // Loopback or the public name, no Origin: the answer is unreadable
    // cross-origin (no CORS), so a read needs no origin check.
    for host in [s.host(), "openwiz.example".to_owned()] {
        let r = s.send(&raw(
            "GET",
            ATTACHMENTS,
            &[("Host".to_owned(), host.clone())],
            b"",
            false,
        ));
        assert_eq!(r.status, 200, "{host}: {}", r.text());
        crate::send_support::assert_no_cors(&host, &r);
    }
    let r = s.send(&raw(
        "GET",
        ATTACHMENTS,
        &[("Host".to_owned(), "elsewhere.example".to_owned())],
        b"",
        false,
    ));
    assert_eq!(r.status, 421);
}

#[test]
fn the_server_reads_no_access_header_or_cookie() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../crates/server/src");
    for entry in std::fs::read_dir(dir).expect("server sources") {
        let path = entry.expect("entry").path();
        let text = std::fs::read_to_string(&path).expect("source");
        for word in [
            "Cf-Access",
            "cf-access",
            "CF_Authorization",
            "Cookie",
            "cookie",
        ] {
            assert!(!text.contains(word), "{path:?} reads {word}");
        }
    }
}
