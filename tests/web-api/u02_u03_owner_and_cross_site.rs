use crate::http::TestServer;
use crate::new_support::{NewFake, PROJECT, TEXT, body, post, preview, serve_new, threads};
use crate::send_support::{HOST, good_headers, with, without};

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
fn u2_no_answer_depends_on_access_headers_or_cookies() {
    let plain = NewFake::new("web_u2_plain");
    let forged_fake = NewFake::new("web_u2_forged");
    let (a, b) = (serve_new(&plain), serve_new(&forged_fake));
    for (n, target) in [threads(PROJECT), preview(PROJECT)].iter().enumerate() {
        let payload = body(&format!("u2-op-{n}"), TEXT).to_string();
        let cases: Vec<(&str, Vec<(String, String)>)> = vec![
            ("same-origin", good_headers()),
            ("no Origin", without(&good_headers(), "Origin")),
            (
                "no custom header",
                without(&good_headers(), "X-OpenWiz-Send"),
            ),
            ("a loopback Host", with(&good_headers(), "Host", &a.host())),
        ];
        for (what, headers) in cases {
            let ra = post(&a, target, &headers, payload.as_bytes());
            let mut forged_headers = forged(&headers);
            if what == "a loopback Host" {
                forged_headers = with(&forged_headers, "Host", &b.host());
            }
            let rb = post(&b, target, &forged_headers, payload.as_bytes());
            assert_eq!(ra.status, rb.status, "{target} {what}");
            assert_eq!(ra.body, rb.body, "{target} {what}: the body differs");
        }
    }
    // The same-origin create went through on both, once each.
    assert_eq!(plain.spawn_count(), 1);
    assert_eq!(forged_fake.spawn_count(), 1);
}

/// One refused form: what it is, its headers, and the expected status and
/// error kind.
type Refusal = (&'static str, Vec<(String, String)>, u16, &'static str);

/// Every cross-site or misdirected form, with its expected status and kind.
fn refusals(s: &TestServer) -> Vec<Refusal> {
    let good = good_headers();
    vec![
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
            "an http Origin",
            with(&good, "Origin", &format!("http://{HOST}")),
            403,
            "forbidden_origin",
        ),
        (
            "an Origin with a port",
            with(&good, "Origin", &format!("https://{HOST}:443")),
            403,
            "forbidden_origin",
        ),
        (
            "Origin null",
            with(&good, "Origin", "null"),
            403,
            "forbidden_origin",
        ),
        (
            "a lookalike Origin",
            with(&good, "Origin", &format!("https://{HOST}.attacker.example")),
            403,
            "forbidden_origin",
        ),
        (
            "Sec-Fetch-Site cross-site",
            with(&good, "Sec-Fetch-Site", "cross-site"),
            403,
            "forbidden_origin",
        ),
        (
            "Sec-Fetch-Site same-site",
            with(&good, "Sec-Fetch-Site", "same-site"),
            403,
            "forbidden_origin",
        ),
        (
            "Sec-Fetch-Site none",
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
            "a wrong custom header",
            with(&good, "X-OpenWiz-Send", "0"),
            403,
            "missing_custom_header",
        ),
        (
            "a form post",
            with(&good, "Content-Type", "application/x-www-form-urlencoded"),
            415,
            "unsupported_media_type",
        ),
        (
            "a multipart post",
            with(&good, "Content-Type", "multipart/form-data; boundary=x"),
            415,
            "unsupported_media_type",
        ),
        (
            "a text/plain post",
            with(&good, "Content-Type", "text/plain"),
            415,
            "unsupported_media_type",
        ),
        (
            "no Content-Type",
            without(&good, "Content-Type"),
            415,
            "unsupported_media_type",
        ),
        (
            "a loopback Host",
            with(&good, "Host", &s.host()),
            421,
            "misdirected",
        ),
        (
            "localhost",
            with(&good, "Host", &format!("localhost:{}", s.addr.port())),
            421,
            "misdirected",
        ),
        (
            "a rebinding Host",
            with(&good, "Host", "attacker.example"),
            421,
            "misdirected",
        ),
    ]
}

#[test]
fn u3_cross_site_and_misdirected_creates_and_previews_are_refused_with_zero_calls() {
    let fake = NewFake::new("web_u3_cross_site");
    let s = serve_new(&fake);
    for target in [threads(PROJECT), preview(PROJECT)] {
        for (n, (what, headers, want, kind)) in refusals(&s).into_iter().enumerate() {
            let op = format!("u3-op-{n}");
            let payload = body(&op, TEXT).to_string();
            let r = post(&s, &target, &headers, payload.as_bytes());
            assert_eq!(r.status, want, "{target} {what}: {}", r.text());
            assert_eq!(r.error().0, kind, "{target} {what}");
            assert!(
                !r.text().contains(TEXT) && !r.text().contains(&op),
                "{what}: the refusal repeats a value"
            );
            assert_eq!(r.headers_named("Access-Control-Allow-Origin"), 0);
            assert!(!fake.record_path(&op).exists(), "{what}");
        }
    }
    assert!(fake.invocations().is_empty(), "{:?}", fake.invocations());
    assert!(!fake.any_record());
}

#[test]
fn u3_without_a_public_host_name_nothing_is_created() {
    let fake = NewFake::new("web_u3_no_host");
    let mut config = crate::new_support::new_config(&fake, openwiz_server::Limits::default());
    config.allowed_host = None;
    if let Some(creating) = config.creating.as_mut() {
        creating.host = None;
    }
    let s = crate::new_support::serve_config(&fake, config);
    for target in [threads(PROJECT), preview(PROJECT)] {
        let r = post(
            &s,
            &target,
            &with(&good_headers(), "Host", &s.host()),
            body("u3-op-host", TEXT).to_string().as_bytes(),
        );
        assert_eq!(r.status, 421, "{target}: {}", r.text());
    }
    assert!(fake.invocations().is_empty());
}

#[test]
fn u3_a_same_origin_create_goes_through_once() {
    let fake = NewFake::new("web_u3_good");
    let s = serve_new(&fake);
    let r = post(
        &s,
        &threads(PROJECT),
        &good_headers(),
        body("u3-op-good", TEXT).to_string().as_bytes(),
    );
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(r.json()["status"], "created", "{}", r.text());
    assert_eq!(fake.spawn_count(), 1);
    // The charset form of the content type is the same request.
    let r = post(
        &s,
        &threads(PROJECT),
        &with(
            &good_headers(),
            "Content-Type",
            "application/json; charset=utf-8",
        ),
        body("u3-op-good", TEXT).to_string().as_bytes(),
    );
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(fake.spawn_count(), 1, "the same op id: replayed");
}
