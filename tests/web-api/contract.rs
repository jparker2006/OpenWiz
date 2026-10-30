//! `contracts/web-api.v1.md` documents every route, parameter, status,
//! error kind, header and limit the tests above check.

use openwiz_server::{
    ALLOWED_HOST_ENV, DEFAULT_PORT, HEAD_TIMEOUT, MAX_CONNECTIONS, MAX_CORE_RUNS, MAX_FILE_BYTES,
    MAX_HEAD_BYTES, MAX_HEADER_LINES, MAX_TARGET_BYTES, MAX_WEB_BYTES, MAX_WEB_FILES, PAGE_CSP,
    REQUEST_DEADLINE, RETRY_AFTER_SECS, ROUTES, SLOT_WAIT, STATIC_CACHE, WEB_ROUTES, WRITE_TIMEOUT,
};

use crate::http::CLI_EXIT_TO_HTTP;

const CONTRACT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../contracts/web-api.v1.md");

#[test]
fn the_contract_documents_what_the_tests_check() {
    let text = std::fs::read_to_string(CONTRACT).expect("contracts/web-api.v1.md");
    let mut missing = Vec::new();
    let mut need = |what: String| {
        if !text.contains(&what) {
            missing.push(what);
        }
    };
    for route in ROUTES {
        need(format!("`{} {}`", route.method, route.path));
    }
    for param in [
        "`limit`",
        "`offset`",
        "`detail`",
        "`hidden`",
        "`archived`",
        "`{id}`",
    ] {
        need(param.to_owned());
    }
    // Every status the server sends, with its error kind, is a row of the
    // contract's status table.
    need("| `200` | (none) |".to_owned());
    for (status, kind) in [
        (400, "bad_request"),
        (400, "invalid_request"),
        (404, "no_route"),
        (404, "not_found"),
        (405, "method_not_allowed"),
        (408, "request_timeout"),
        (414, "target_too_long"),
        (421, "misdirected"),
        (431, "head_too_large"),
        (500, "internal"),
        (502, "read_failed"),
        (503, "busy"),
        (504, "deadline"),
        (505, "http_version"),
    ] {
        need(format!("| `{status}` | `{kind}` |"));
    }
    for (exit, status, kind) in CLI_EXIT_TO_HTTP {
        need(format!("| {exit} | `{status}` | `{kind}` |"));
    }
    for header in [
        "`Content-Type: application/json`",
        "`Cache-Control: no-store`",
        "`X-Content-Type-Options: nosniff`",
        "`Referrer-Policy: no-referrer`",
        "`X-Frame-Options: DENY`",
        "`Content-Security-Policy: default-src 'none'; frame-ancestors 'none'`",
        "`Connection: close`",
        "`Allow: GET`",
        "`Retry-After: 5`",
    ] {
        need(header.to_owned());
    }
    for value in [
        format!("{MAX_CONNECTIONS} connections"),
        format!("{MAX_CORE_RUNS} core runs"),
        format!("{} seconds for a core run slot", SLOT_WAIT.as_secs()),
        format!("`Retry-After: {RETRY_AFTER_SECS}`"),
        format!(
            "{} seconds from the end of the request head",
            REQUEST_DEADLINE.as_secs()
        ),
        format!(
            "{} seconds for the whole request head",
            HEAD_TIMEOUT.as_secs()
        ),
        format!("{} seconds for the whole response", WRITE_TIMEOUT.as_secs()),
        format!("{MAX_TARGET_BYTES} bytes"),
        format!("{} KiB", MAX_HEAD_BYTES / 1024),
        format!("{MAX_HEADER_LINES} header lines"),
        format!("`{ALLOWED_HOST_ENV}`"),
        format!("`{DEFAULT_PORT}`"),
        "openwiz.inventory.v1".to_owned(),
        "contracts/inventory.v1.schema.json".to_owned(),
    ] {
        need(value);
    }
    // A8: the web page.
    need("`--web-root`".to_owned());
    need("`Allow: GET, HEAD`".to_owned());
    need(format!("`Content-Security-Policy: {PAGE_CSP}`"));
    need(format!("`{STATIC_CACHE}`"));
    need(format!("{} MiB per file", MAX_FILE_BYTES / (1024 * 1024)));
    need(format!("{MAX_WEB_FILES} files"));
    need(format!("{} MiB in all", MAX_WEB_BYTES / (1024 * 1024)));
    need("No request touches the filesystem".to_owned());
    need("restart the server".to_owned());
    for route in WEB_ROUTES {
        need(format!("| `{route}` |"));
    }
    for (extension, content_type) in [
        ("html", "text/html; charset=utf-8"),
        ("js", "text/javascript; charset=utf-8"),
        ("css", "text/css; charset=utf-8"),
        ("woff2", "font/woff2"),
        ("txt", "text/plain; charset=utf-8"),
    ] {
        need(format!("| `.{extension}` | `{content_type}` |"));
    }
    assert!(
        missing.is_empty(),
        "the contract does not document: {missing:#?}"
    );
}

/// The `room-cli` version is never a gate, so no status or
/// CLI exit is documented for an untested version any more.
#[test]
fn the_contract_has_no_untested_version_outcome() {
    let text = std::fs::read_to_string(CONTRACT).expect("contracts/web-api.v1.md");
    assert!(
        !text.contains("`untested_version`"),
        "contracts/web-api.v1.md still documents an untested_version outcome"
    );
}
