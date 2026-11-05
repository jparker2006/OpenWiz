use crate::msg::SendFake;
use crate::send_support::{body, good_headers, post, serve_send, with, without};

/// Forged Access material a request could carry. None of it is real.
fn forged(headers: &[(String, String)]) -> Vec<(String, String)> {
    let mut out = with(
        headers,
        "Cf-Access-Jwt-Assertion",
        "eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiJzeW50aGV0aWMifQ.c3ludGhldGlj",
    );
    out = with(
        &out,
        "Cf-Access-Authenticated-User-Email",
        "owner@example.invalid",
    );
    with(
        &out,
        "Cookie",
        "CF_Authorization=synthetic; CF_Binding=synthetic",
    )
}

#[test]
fn no_answer_depends_on_access_headers_or_cookies() {
    let plain = SendFake::new("web_w2_plain", "send");
    let forged_fake = SendFake::new("web_w2_forged", "send");
    let (a, b) = (serve_send(&plain), serve_send(&forged_fake));
    let payload = body("w2-op-1", "queue", "Synthetic W2 text").to_string();
    let cases: Vec<(&str, Vec<(String, String)>)> = vec![
        ("a same-origin send", good_headers()),
        ("no Origin", without(&good_headers(), "Origin")),
        (
            "no custom header",
            without(&good_headers(), "X-OpenWiz-Send"),
        ),
        ("a loopback Host", with(&good_headers(), "Host", &a.host())),
    ];
    for (what, headers) in cases {
        let ra = post(&a, "thr_syn_0001", &headers, payload.as_bytes());
        let mut forged_headers = forged(&headers);
        if what == "a loopback Host" {
            forged_headers = with(&forged_headers, "Host", &b.host());
        }
        let rb = post(&b, "thr_syn_0001", &forged_headers, payload.as_bytes());
        assert_eq!(ra.status, rb.status, "{what}");
        assert_eq!(ra.body, rb.body, "{what}: the body differs");
    }
    // The same-origin send went through on both, once each.
    assert_eq!(plain.tell_count(), 1);
    assert_eq!(forged_fake.tell_count(), 1);
}

#[test]
fn the_server_never_reads_access_headers_or_cookies() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../crates/server/src");
    for entry in std::fs::read_dir(dir).expect("server sources") {
        let path = entry.expect("entry").path();
        let text = std::fs::read_to_string(&path)
            .expect("source")
            .to_ascii_lowercase();
        for word in ["cf-access", "cf_authorization", "cf_binding", "\"cookie\""] {
            assert!(!text.contains(word), "{path:?} mentions {word}");
        }
    }
}
