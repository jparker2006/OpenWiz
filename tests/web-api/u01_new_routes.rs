//! U1 : exactly the new routes. The route table is 0.0.3's plus
//! the Cloud options and the status (`GET`), and the create and the
//! new-thread preview (`POST`): the create is the only new write. There is
//! no retry route (sheet the check). Every other method on a new route is `405`,
//! and none of them reads, claims or creates anything.

use openwiz_server::{ROUTES, Route};

use crate::new_support::{
    NewFake, OPTIONS, PROJECT, TEXT, body, create, new_config, preview, serve_config, serve_new,
    status, threads,
};
use crate::send_support::{good_headers, raw};

/// The route table A2 adds to W1's, in order: two reads and two `POST`s, of
/// which only the create writes.
pub const NEW_ROUTES: &[Route] = &[
    Route {
        method: "GET",
        path: "/api/v1/cloud-options",
    },
    Route {
        method: "GET",
        path: "/api/v1/new-threads/{op_id}",
    },
    Route {
        method: "POST",
        path: "/api/v1/projects/{id}/threads/preview",
    },
    Route {
        method: "POST",
        path: "/api/v1/projects/{id}/threads",
    },
];

#[test]
fn the_route_table_adds_exactly_the_new_thread_routes() {
    let after = crate::aw01_routes::ATTACHMENT_ROUTES.len();
    let end = ROUTES.len() - after;
    assert_eq!(&ROUTES[end - NEW_ROUTES.len()..end], NEW_ROUTES);
    assert_eq!(ROUTES.len(), 6 + NEW_ROUTES.len() + after, "{ROUTES:?}");
    for route in ROUTES {
        assert!(
            !route.path.contains("retry"),
            "no retry route (A-3): {route:?}"
        );
    }
}

#[test]
fn every_other_method_on_the_post_routes_is_405_and_creates_nothing() {
    let fake = NewFake::new("web_u1_post_methods");
    let s = serve_new(&fake);
    let payload = body("u1-op-1", TEXT).to_string();
    for target in [threads(PROJECT), preview(PROJECT)] {
        for method in [
            "GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "CONNECT", "TRACE", "post", "X",
        ] {
            let r = s.send(&raw(
                method,
                &target,
                &good_headers(),
                payload.as_bytes(),
                true,
            ));
            assert_eq!(r.status, 405, "{method} {target}: {}", r.status_line);
            assert_eq!(r.header("Allow"), Some("POST"), "{method} {target}");
        }
    }
    assert!(fake.invocations().is_empty(), "{:?}", fake.invocations());
    assert!(!fake.any_record());
}

#[test]
fn post_on_the_read_routes_is_405_and_reads_nothing() {
    let fake = NewFake::new("web_u1_read_methods");
    let s = serve_new(&fake);
    let payload = body("u1-op-2", TEXT).to_string();
    for target in [OPTIONS.to_owned(), status("u1-op-2")] {
        for method in ["POST", "PUT", "DELETE", "PATCH", "OPTIONS"] {
            let r = s.send(&raw(
                method,
                &target,
                &good_headers(),
                payload.as_bytes(),
                true,
            ));
            assert_eq!(r.status, 405, "{method} {target}: {}", r.status_line);
            assert_eq!(r.header("Allow"), Some("GET"), "{method} {target}");
        }
    }
    assert!(fake.invocations().is_empty(), "{:?}", fake.invocations());
    assert!(!fake.any_record());
}

#[test]
fn there_is_no_retry_route_for_any_method() {
    let fake = NewFake::new("web_u1_no_retry");
    let s = serve_new(&fake);
    // A recorded create to retry, were there a way.
    let r = create(&s, PROJECT, &body("u1-op-3", TEXT));
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(fake.spawn_count(), 1);
    let payload = body("u1-op-3", TEXT).to_string();
    for target in [
        "/api/v1/new-threads/u1-op-3/retry",
        "/api/v1/new-threads/u1-op-3/",
        "/api/v1/projects/prj_syn_0001/threads/u1-op-3/retry",
        "/api/v1/projects/prj_syn_0001/threads/retry",
        "/api/v1/projects/prj_syn_0001",
        "/api/v1/projects",
        "/api/v1/threads",
        "/api/v1/new-threads",
    ] {
        // No route: by the contract's order (the method, then the route), a
        // `GET` is `404` and anything else `405` with `Allow: GET`.
        for (method, want) in [("POST", 405), ("PUT", 405), ("GET", 404)] {
            let payload: &[u8] = if method == "GET" {
                b""
            } else {
                payload.as_bytes()
            };
            let r = s.send(&raw(method, target, &good_headers(), payload, true));
            assert_eq!(r.status, want, "{method} {target}: {}", r.status_line);
            let allow = (want == 405).then_some("GET");
            assert_eq!(r.header("Allow"), allow, "{method} {target}");
        }
    }
    assert_eq!(fake.spawn_count(), 1, "nothing was created again");
}

#[test]
fn without_a_new_thread_configuration_the_routes_are_absent_for_every_method() {
    let fake = NewFake::new("web_u1_disabled");
    let mut config = new_config(&fake, openwiz_server::Limits::default());
    config.creating = None;
    let s = serve_config(&fake, config);
    let payload = body("u1-op-4", TEXT).to_string();
    for target in [
        threads(PROJECT),
        preview(PROJECT),
        OPTIONS.to_owned(),
        status("u1-op-4"),
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
        }
    }
    assert!(fake.invocations().is_empty(), "{:?}", fake.invocations());
}

#[test]
fn get_on_the_reads_never_creates() {
    let fake = NewFake::new("web_u1_reads");
    let s = serve_new(&fake);
    for target in [OPTIONS.to_owned(), status("u1-op-5")] {
        let r = s.get(&target);
        assert_eq!(r.status, 200, "{target}: {}", r.text());
    }
    let r = crate::new_support::preview_json(&s, PROJECT, &body("u1-op-5", TEXT));
    assert_eq!(r.status, 200, "{}", r.text());
    assert!(fake.writes().is_empty(), "{:?}", fake.writes());
    assert!(!fake.any_record(), "no read claims or records anything");
    // And the one create still works, once.
    let r = create(&s, PROJECT, &body("u1-op-5", TEXT));
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(fake.spawn_count(), 1);
}
