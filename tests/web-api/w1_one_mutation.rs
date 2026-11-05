use openwiz_server::{ROUTES, Route};

use crate::msg::SendFake;
use crate::send_support::{HOST, body, good_headers, messages, raw, send_json, serve_send, with};

#[test]
fn the_route_table_is_the_read_routes_and_one_post() {
    let new = crate::u01_new_routes::NEW_ROUTES;
    let attachments = crate::aw01_routes::ATTACHMENT_ROUTES;
    let first = ROUTES.len() - new.len() - attachments.len();
    assert_eq!(&ROUTES[first..first + new.len()], new);
    assert_eq!(&ROUTES[first + new.len()..], attachments);
    assert_eq!(
        &ROUTES[..first],
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
                path: "/api/v1/sessions/{id}/preview",
            },
            Route {
                method: "GET",
                path: "/api/v1/sessions/{id}/messages/{op_id}",
            },
            Route {
                method: "GET",
                path: "/api/v1/health",
            },
            Route {
                method: "POST",
                path: "/api/v1/sessions/{id}/messages",
            },
        ][..]
    );
    let sends: Vec<&Route> = ROUTES
        .iter()
        .filter(|r| r.method != "GET" && r.path.starts_with("/api/v1/sessions/"))
        .collect();
    assert_eq!(sends.len(), 1, "{sends:?}");
}

#[test]
fn every_other_method_on_the_send_route_is_405_and_sends_nothing() {
    let fake = SendFake::new("web_w1_methods", "send");
    let s = serve_send(&fake);
    let payload = body("w1-op-1", "queue", "Synthetic W1 text").to_string();
    for method in [
        "GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "CONNECT", "TRACE", "post", "Post", "X",
    ] {
        let r = s.send(&raw(
            method,
            &messages("thr_syn_0001"),
            &good_headers(),
            payload.as_bytes(),
            true,
        ));
        assert_eq!(r.status, 405, "{method}: {}", r.status_line);
        assert_eq!(r.header("Allow"), Some("POST"), "{method}");
    }
    assert_eq!(fake.tell_count(), 0);
    assert!(fake.invocations().is_empty(), "{:?}", fake.invocations());
    assert!(!fake.record_path("w1-op-1").exists());
}

#[test]
fn post_anywhere_else_is_405_and_sends_nothing() {
    let fake = SendFake::new("web_w1_elsewhere", "send");
    let s = serve_send(&fake);
    let payload = body("w1-op-2", "queue", "Synthetic W1 text").to_string();
    for target in [
        "/api/v1/inventory",
        "/api/v1/sessions/thr_syn_0001",
        "/api/v1/sessions/thr_syn_0001/preview",
        "/api/v1/sessions/thr_syn_0001/messages/w1-op-2",
        "/api/v1/health",
        "/api/v1/sessions/thr_syn_0001/messages/",
        "/api/v1/sessions/thr_syn_0001/message",
        "/api/v1/messages",
        "/",
        "/nowhere",
    ] {
        let r = s.send(&raw(
            "POST",
            target,
            &good_headers(),
            payload.as_bytes(),
            true,
        ));
        assert_eq!(r.status, 405, "{target}: {}", r.status_line);
        assert_eq!(r.header("Allow"), Some("GET"), "{target}");
    }
    assert_eq!(fake.tell_count(), 0);
    assert!(fake.invocations().is_empty(), "{:?}", fake.invocations());
}

#[test]
fn get_never_sends() {
    let fake = SendFake::new("web_w1_get", "send");
    let s = serve_send(&fake);
    for target in [
        "/api/v1/sessions/thr_syn_0001/preview",
        "/api/v1/sessions/thr_syn_0001/preview?mode=steer",
        "/api/v1/sessions/thr_syn_0001/messages/w1-op-3",
    ] {
        let r = s.send(&raw(
            "GET",
            target,
            &with(&good_headers(), "Host", HOST),
            b"",
            false,
        ));
        assert_eq!(r.status, 200, "{target}: {}", r.text());
    }
    assert_eq!(fake.tell_count(), 0);
    assert!(fake.tells().is_empty());
    // And the one write still works, once.
    let r = send_json(
        &s,
        "thr_syn_0001",
        &body("w1-op-3", "queue", "Synthetic W1 text"),
    );
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(fake.tell_count(), 1);
}

#[test]
fn without_sending_the_send_routes_are_absent_for_every_method() {
    let fake = SendFake::new("web_w1_disabled", "send");
    let mut config = crate::send_support::send_config(&fake, openwiz_server::Limits::default());
    config.sending = None;
    let s = crate::send_support::serve_config(&fake, config);
    let payload = body("w1-op-9", "queue", "Synthetic W1 text").to_string();
    for target in [
        messages("thr_syn_0001"),
        "/api/v1/sessions/thr_syn_0001/preview".to_owned(),
        "/api/v1/sessions/thr_syn_0001/messages/w1-op-9".to_owned(),
    ] {
        for method in ["GET", "POST", "OPTIONS", "HEAD", "PUT", "DELETE"] {
            let r = s.send(&raw(
                method,
                &target,
                &good_headers(),
                payload.as_bytes(),
                true,
            ));
            assert_eq!(r.status, 404, "{method} {target}: {}", r.status_line);
            assert_eq!(r.header("Allow"), None, "{method} {target}");
            if method == "HEAD" {
                assert!(r.body.is_empty(), "{method} {target}");
            } else {
                assert_eq!(r.error().0, "no_route", "{method} {target}");
            }
        }
    }
    assert_eq!(fake.tell_count(), 0);
    assert!(fake.invocations().is_empty(), "{:?}", fake.invocations());
    assert!(!fake.state.exists(), "the send record was touched");
}
