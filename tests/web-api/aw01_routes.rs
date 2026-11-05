//! AW1: exactly the planned routes. The route table is A2's plus
//! the three attachment routes: the list (`GET`), the upload (`POST`) and
//! the removal (`DELETE`). Every other method and path is refused, and
//! stages nothing. **No route returns an attachment's bytes**, whatever is
//! asked: a marker uploaded in a file never comes back in any answer.

use openwiz_server::{Limits, ROUTES, Route};

use crate::attach_support::{
    ATTACHMENTS, Free, State, attach_config, attachment, list, ready_id, remove_headers, serve,
    serve_attach, upload, upload_headers,
};
use crate::send_support::{HOST, good_headers, raw};

/// The route table B3 adds to A2's, in order (U1's `NEW_ROUTES` before it).
pub const ATTACHMENT_ROUTES: &[Route] = &[
    Route {
        method: "GET",
        path: "/api/v1/attachments",
    },
    Route {
        method: "POST",
        path: "/api/v1/attachments",
    },
    Route {
        method: "DELETE",
        path: "/api/v1/attachments/{id}",
    },
];

#[test]
fn the_route_table_adds_exactly_the_attachment_routes() {
    assert_eq!(
        &ROUTES[ROUTES.len() - ATTACHMENT_ROUTES.len()..],
        ATTACHMENT_ROUTES
    );
    let new = crate::u01_new_routes::NEW_ROUTES;
    let before = ROUTES.len() - ATTACHMENT_ROUTES.len();
    assert_eq!(&ROUTES[before - new.len()..before], new);
    assert_eq!(
        ROUTES.len(),
        6 + new.len() + ATTACHMENT_ROUTES.len(),
        "{ROUTES:?}"
    );
    for route in ROUTES {
        for word in ["content", "bytes", "download", "raw", "file", "preview/"] {
            assert!(
                !route.path.contains(word),
                "no route serves an attachment: {route:?}"
            );
        }
    }
}

/// A marker that only an uploaded file holds.
const MARKER: &[u8] = b"SYNTHETIC-AW1-MARKER-7f3a";

fn marked(prefix: &[u8]) -> Vec<u8> {
    let mut bytes = prefix.to_vec();
    bytes.extend_from_slice(MARKER);
    bytes.extend_from_slice(b"\n");
    bytes
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn every_other_method_on_the_attachment_routes_is_405_and_stages_nothing() {
    let state = State::new("web_aw1_methods");
    let s = serve_attach(&state, Free::plenty(), Limits::default());
    let payload = marked(b"Synthetic AW1 text ");
    for method in [
        "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "CONNECT", "TRACE", "post", "get", "X",
    ] {
        let r = s.send(&raw(
            method,
            ATTACHMENTS,
            &upload_headers("a.txt"),
            &payload,
            true,
        ));
        assert_eq!(r.status, 405, "{method}: {}", r.status_line);
        assert_eq!(r.header("Allow"), Some("GET, POST"), "{method}");
    }
    for method in [
        "GET", "POST", "PUT", "PATCH", "HEAD", "OPTIONS", "TRACE", "delete", "X",
    ] {
        let target = attachment("att-0123456789abcdef0123456789abcdef");
        let r = s.send(&raw(
            method,
            &target,
            &upload_headers("a.txt"),
            &payload,
            true,
        ));
        assert_eq!(r.status, 405, "{method} {target}: {}", r.status_line);
        assert_eq!(r.header("Allow"), Some("DELETE"), "{method} {target}");
    }
    assert!(state.nothing_staged(), "{:?}", state.entries());
}

#[test]
fn no_route_returns_an_attachments_bytes() {
    let state = State::new("web_aw1_no_bytes");
    let s = serve_attach(&state, Free::plenty(), Limits::default());
    let svg = marked(b"<svg xmlns=\"http://www.w3.org/2000/svg\"><!-- ");
    let text_file = marked(b"Synthetic AW1 text ");
    let mut answers = vec![];
    let r = upload(&s, "aw1.txt", &text_file);
    let text_id = ready_id(&r);
    answers.push(r);
    let r = upload(&s, "aw1.svg", &svg);
    let svg_id = ready_id(&r);
    answers.push(r);
    answers.push(list(&s));
    // Every way of asking for one, by its id, under every path that could
    // look like it serves a file.
    for id in [&text_id, &svg_id] {
        for target in [
            attachment(id),
            format!("{}/content", attachment(id)),
            format!("{}/bytes", attachment(id)),
            format!("{}/raw", attachment(id)),
            format!("{}/aw1.txt", attachment(id)),
            format!("{ATTACHMENTS}?id={id}"),
            format!("{ATTACHMENTS}?ids={id}"),
            format!("/api/v1/attachments/{id}/"),
            format!("/attachments/{id}"),
            format!("/{id}"),
            format!("/api/v1/{id}"),
        ] {
            for method in ["GET", "HEAD", "POST"] {
                let body: &[u8] = if method == "POST" { b"{}" } else { b"" };
                let headers = if method == "POST" {
                    good_headers()
                } else {
                    vec![("Host".to_owned(), HOST.to_owned())]
                };
                answers.push(s.send(&raw(method, &target, &headers, body, method == "POST")));
            }
        }
    }
    for r in &answers {
        assert!(
            !contains(&r.body, MARKER),
            "an answer carries an uploaded file's bytes: {} {}",
            r.status_line,
            String::from_utf8_lossy(&r.body)
        );
        assert_ne!(
            r.header("Content-Type"),
            Some("image/svg+xml"),
            "{}",
            r.status_line
        );
        if r.status == 200 {
            assert_eq!(r.header("Content-Type"), Some("application/json"));
        }
    }
    // The bytes are still there: only the answers lack them.
    assert_eq!(state.entries().len(), 2);
}

#[test]
fn without_an_attachment_configuration_the_routes_are_absent_for_every_method() {
    let state = State::new("web_aw1_disabled");
    let mut config = attach_config(&state.dir, Free::plenty(), Limits::default());
    config.attaching = None;
    let fake = crate::fake::Fake::new("web_aw1_disabled_source", "normal");
    let s = serve(
        config,
        std::sync::Arc::new(fake.source()),
        std::sync::Arc::new(crate::new_support::clock()),
    );
    for target in [
        ATTACHMENTS.to_owned(),
        attachment("att-0123456789abcdef0123456789abcdef"),
    ] {
        for method in ["GET", "POST", "DELETE", "PUT", "OPTIONS", "HEAD"] {
            let r = s.send(&raw(method, &target, &upload_headers("a.txt"), b"x", true));
            assert_eq!(r.status, 404, "{method} {target}: {}", r.status_line);
            assert_eq!(r.header("Allow"), None, "{method} {target}");
        }
    }
    assert!(state.nothing_staged());
}

#[test]
fn the_list_and_the_removal_take_no_query_and_the_removal_no_body() {
    let state = State::new("web_aw1_shapes");
    let s = serve_attach(&state, Free::plenty(), Limits::default());
    let id = ready_id(&upload(&s, "aw1.txt", b"Synthetic AW1 text\n"));
    let r = crate::send_support::get_public(&s, &format!("{ATTACHMENTS}?all=1"));
    assert_eq!(r.status, 400, "{}", r.text());
    assert_eq!(r.error().0, "invalid_request");
    let r = s.send(&raw(
        "DELETE",
        &format!("{}?force=1", attachment(&id)),
        &remove_headers(),
        b"",
        false,
    ));
    assert_eq!(r.status, 400, "{}", r.text());
    assert_eq!(r.error().0, "invalid_request");
    let r = s.send(&raw(
        "DELETE",
        &attachment(&id),
        &remove_headers(),
        b"{}",
        true,
    ));
    assert_eq!(r.status, 400, "{}", r.text());
    assert_eq!(r.error().0, "bad_request");
    let mut chunked = remove_headers();
    chunked.push(("Transfer-Encoding".to_owned(), "chunked".to_owned()));
    let r = s.send(&raw(
        "DELETE",
        &attachment(&id),
        &chunked,
        b"0\r\n\r\n",
        false,
    ));
    assert_eq!(r.status, 400, "{}", r.text());
    // Still there after every refusal.
    assert_eq!(state.entries().len(), 1);
    // A removal with an empty body (`Content-Length: 0`) is a removal.
    let r = s.send(&raw(
        "DELETE",
        &attachment(&id),
        &remove_headers(),
        b"",
        true,
    ));
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(r.json()["ids"][0]["result"], "removed");
    assert!(state.nothing_staged());
}
