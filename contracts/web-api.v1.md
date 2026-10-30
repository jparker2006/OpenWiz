# OpenWiz web API, version 1

The HTTP API of `openwiz-server` (`crates/server`), for OpenWiz 0.0.2. It is a thin client of the core, like the `openwiz` CLI: each route calls the same core operation with the same options, and each success body is the `openwiz.inventory.v1` envelope that `openwiz … --json` prints. The envelope's schema is [`contracts/inventory.v1.schema.json`](inventory.v1.schema.json), reused unchanged. Nothing here adds a field, an operation or a rule.

**Status:** implemented and proven on fixtures (`tests/web-api`), the web page (A8) included. What was checked live, and what was not, is in [`docs/evidence/0.0.2-live-check.md`](../docs/evidence/0.0.2-live-check.md); the access path (Cloudflare Tunnel and Access) is set up as in [`docs/web-access.md`](../docs/web-access.md).

## Transport

- **Loopback only.** The server listens on `127.0.0.1:<port>` and nothing else; no option chooses another address. The port is `--port N`, default `7370` (provisional); `--port 0` picks a free one. Nothing opens an inbound port.
- **HTTP/1.1 and HTTP/1.0 over plain TCP.** One request per connection: every response carries `Connection: close`. No keep-alive, no pipelining, no `Upgrade` (a WebSocket request gets an ordinary response), no server-sent events, no compression.
- **`GET` only** on the API, and **`GET` and `HEAD`** on the web page's paths when `--web-root` is given ([below](#the-web-page---web-root)). No request body is accepted.
- **Same origin.** No CORS header is ever sent, so a browser page on another origin cannot read a response.

## Routes

| Route | Same as | Success body |
| --- | --- | --- |
| `GET /api/v1/inventory` | `openwiz ls` or `ls -l`, with the query below | The envelope, as `ls --json` or `ls -l --json` prints it |
| `GET /api/v1/sessions/{id}` | `openwiz show <id> --json` | The envelope, as `show --json` prints it |
| `GET /api/v1/health` | Nothing: no `room-cli` read | `{"server": {"name": "openwiz-server", "version": "<crate version>"}, "source": {"name": "room-cli", "tested_version": "<the version last live-tested: evidence, not a gate>"}}` |

Paths match exactly, case-sensitive, with no trailing slash and no percent-decoding. Any other path is `404` (`no_route`), except the web page's files when `--web-root` is given. Any method other than `GET`, on any other path, is `405` with `Allow: GET`. That includes `HEAD`; only the web page's paths take it. As HTTP requires, a response to a request that is, or may be, a `HEAD` request has the usual headers but **no body and no `Content-Length`** (which would have to describe a `GET`'s body: RFC 9110, 8.6 and 9.3.2), whatever its status: a web file's `200`, a `404` or `405`, an earlier refusal (`400`, `408`, `414`, `421`, `431`, `505`), or the connection cap's `503`. "May be" means its first bytes, as far as they arrived, are `HEAD ` or a non-empty beginning of it (`H`, `HE`, `HEA`, `HEAD`): over the connection cap the server peeks at them for up to 200 ms; otherwise they are the bytes read before the refusal. A response to a request none of whose bytes arrived, or whose first bytes cannot begin `HEAD `, has its body and `Content-Length` as usual. There are no write, launch, message or approval routes.

## The web page (`--web-root`)

`openwiz-server --web-root DIR` also serves `apps/web`'s static export (`npm run build` makes `apps/web/out`) on the same origin as the API. **Without `--web-root`, every non-API path is `404`**, as before.

- **One directory, loaded once, at start.** `DIR` is given at most once and must be a directory holding a regular `index.html` (a symlink in `DIR` itself is resolved once, at start). Before listening, the server walks all of `DIR` and reads its files into memory. It refuses the whole directory, and does not start (exit 1, with a message that names the rule, never the path), if any entry anywhere under it:
  - is a symlink, even one pointing inside `DIR`;
  - is not a regular file or a directory (a FIFO, a socket, a device);
  - has a name outside letters, digits, `.`, `_` and `-`, or starting with `.` (so `.DS_Store` is refused: delete it);
  - is a file with an extension outside the type table below, with more than one link (a hard link), or on another device than `DIR`;
  - is a file over 1 MiB; or if there are more than 512 files, more than 8 MiB in all, or directories nested more than 16 deep.

  **No request touches the filesystem.** Every answer comes from the files loaded at start, so replacing, changing or deleting `DIR`, an ancestor or a file afterwards changes nothing that is served. **To serve a new export, restart the server.** The one open of each file happens at start, just after its type is checked; a file swapped for a FIFO at that moment could delay the start, never a request.
- **An allowlist that matches the export.** Only three web paths, with these log routes:

  | Request path | Served file | Log route | `Cache-Control` |
  | --- | --- | --- | --- |
  | `/` | `DIR/index.html` | `/` | `no-store` |
  | `/_next/static/<path>` | `DIR/_next/static/<path>` | `/_next/static/*` | `private, max-age=31536000, immutable` |
  | `/fonts/<path>` | `DIR/fonts/<path>` | `/fonts/*` | `private, max-age=31536000, immutable` |

  Nothing else under `DIR` is served: not `/index.html`, `/404.html`, `/_next/<build id>/…`, nor any other top-level file. Any other path is the API's `404` (`no_route`).
- **Containment.** A request is matched exactly against the loaded paths: nothing is percent-decoded, resolved or normalised, so `..`, `.`, `//`, a trailing `/`, `\`, `%2e%2e` and every other path not loaded are `404` (`no_route`). There is no directory listing and no index other than `/`.
- **A fixed table of content types.** A file with any other extension anywhere under `DIR` stops the start:

  | Extension | `Content-Type` |
  | --- | --- |
  | `.html` | `text/html; charset=utf-8` |
  | `.js` | `text/javascript; charset=utf-8` |
  | `.css` | `text/css; charset=utf-8` |
  | `.woff2` | `font/woff2` |
  | `.txt` | `text/plain; charset=utf-8` |

- **Methods.** `GET` and `HEAD` only; any other method on a web path is `405` with `Allow: GET, HEAD`. A response to `HEAD` has `GET`'s status and headers, without a body or `Content-Length`, as above. A web path takes no query (`400`, `invalid_request`) and no body (`400`, `bad_request`). The `Host` check comes first, as for the API.
- **Headers on a web file** (`200`), separate from the API's:
  - `Content-Type` from the table, and `Cache-Control` from the route table;
  - `Content-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self'; font-src 'self'; img-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'` (the web page's own policy: no inline script or style, no `unsafe-*`, nonce or hash);
  - `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer`, `X-Frame-Options: DENY`, `Connection: close` and `Content-Length`;
  - no CORS header, `Set-Cookie`, `Server`, `ETag` or `Last-Modified`.

  Every refusal on a web path (`400`, `404`, `405`, `421`, …) is the API's JSON error with the API's headers.
- **Caching** is a default, for now: the hashed assets and fonts are cached by the user's own browser for a year (`private`: never by a shared cache such as Cloudflare's edge), and the page is never stored, so a new build is picked up on the next load. The fonts are not content-hashed: a changed font needs a new file name.

### `GET /api/v1/inventory` query

Every parameter is optional and may appear at most once. Values are exact: no sign, space, decimal point or percent-encoding. A malformed, unknown or repeated parameter is `400` (`invalid_request`), and nothing is read.

| Parameter | Values | Default (the CLI's) | CLI equivalent |
| --- | --- | --- | --- |
| `detail` | `1` or `2` | `1` | `1` is `ls`; `2` is `ls -l` |
| `hidden` | `0` or `1` | `0` | `1` is `--hidden` |
| `archived` | `0` or `1` | `0` | `1` is `-a` |
| `limit` | A whole number, `0` and up (decimal digits; leading zeros allowed) | `100` (the core's `DEFAULT_LIMIT`) | `--limit N` |
| `offset` | A whole number, `0` and up (decimal digits; leading zeros allowed) | `0` | `--offset N` |

The server defines no inventory limit of its own. The cap on `limit` (the core's `MAX_LIMIT`, 200) is the core's check, with the core's message (such as `--limit 201 is above the cap of 200`), before any read. An empty query (`/api/v1/inventory?`) is the defaults.

### `GET /api/v1/sessions/{id}`

`{id}` is one path segment, passed to the core as it is. The id rule is the adapter's (1 to 128 ASCII letters, digits, `_` or `-`, not starting with `-`). An id it refuses is `400` (`invalid_request`) with the core's message; as with `openwiz show`, `room-cli --version` is read first. This route and `/api/v1/health` take no query: any `?` is `400`.

## Outcomes

Every core outcome maps to one status. The message is the CLI's own text, the one it prints after `openwiz: `, without the CLI's terminal cleaning: JSON escapes control characters instead, and clients must render it as plain text.

| CLI exit | Status | `error.kind` | Core outcome |
| --- | --- | --- | --- |
| 0 | `200` | (none) | A result, complete or partial. Partial coverage is still `200`: the envelope's `coverage` and `gaps` say what is missing |
| 1 | `502` | `read_failed` | The session list, or the session `show` asked for, could not be read |
| 2 | `400` | `invalid_request` | The request is out of the core's bounds (a `limit` above the cap, an id the adapter refuses) |
| 3 | `404` | `not_found` | `show`: the session was not found |

### Error body

Every error, from the core or from the server, is JSON (except the two bodyless answers: a response to `HEAD`, and the accept loop's `503` past both connection caps):

```json
{
  "error": {
    "kind": "not_found",
    "message": "session thr_syn_9999 not found"
  }
}
```

The server's own errors (every error that is not a core outcome, including a bad query's `invalid_request`) never repeat the request's values and carry no file path, environment, stack trace or credential. A core outcome's message is the CLI's: `not_found`, and the core's `invalid_request` for a refused id, can name the id asked for, and a failed read's message carries `room-cli`'s first non-empty stderr line only if that line is in the adapter's safe grammar (`Error: HTTP <code>`, optionally `: ` and a reason phrase of ASCII letters and single spaces, at most 60 characters). Otherwise a line containing `HTTP 404: Thread not found` becomes `Error: HTTP 404: Thread not found (detail withheld)` (and a detail read reports that session as not found); any other line keeps only its status, as `Error: HTTP <code> (detail withheld)`, or nothing of it, as `room-cli error (detail withheld)`; an empty stderr is `no error output`. Text `room-cli` prints where a version, JSON or a recorded value is expected is withheld the same way. The same rule holds for a success body's envelope, in `gaps` and in `unknown (…)` reasons, as in `openwiz … --json`. The rule is the adapter's (`crates/adapters/cloudroom`), not the server's; the server adds to the core's text only what the CLI adds: the source's fixed read name (`thread list`, `thread show`) before a failed read. The `room-cli` version is never an error: any version is read, shown in the envelope's `source`, and the run goes on. A letters-only word in a safe line (a name, say) can still be shown, and session content (titles, purposes, final output, a recorded error) is data, shown by design.

### Every status

| Status | `error.kind` | When |
| --- | --- | --- |
| `200` | (none) | A result, or the health body |
| `400` | `bad_request` | A malformed request line or header, CRLF missing, a leading blank line, a missing or repeated `Host`, a malformed `Host`, any request body (`Content-Length` above 0, or any `Transfer-Encoding`) |
| `400` | `invalid_request` | A malformed, unknown or repeated query parameter; a query on a route that takes none; the core's `InvalidRequest` |
| `404` | `no_route` | Any path other than the three routes, or, with `--web-root`, a web path that is not served |
| `404` | `not_found` | `show` of a session the source does not know |
| `405` | `method_not_allowed` | Any method other than `GET`; with `Allow: GET`. On a web path, any method other than `GET` and `HEAD`; with `Allow: GET, HEAD` |
| `408` | `request_timeout` | The request head did not arrive within its time |
| `414` | `target_too_long` | The request target is longer than its cap |
| `421` | `misdirected` | The `Host` is not this server's loopback address or the configured host name |
| `431` | `head_too_large` | The request head, or its number of header lines, is over its cap |
| `500` | `internal` | A core run failed unexpectedly, or could not be started. Not expected |
| `502` | `read_failed` | As above |
| `503` | `busy` | Over the connection cap, or no core run slot within its wait; with `Retry-After: 5` |
| `504` | `deadline` | No result within the request deadline (only if a source ignores the core's budget) |
| `505` | `http_version` | An HTTP version other than 1.1 and 1.0 |

The checks run in this order. While the head arrives: `408` when it is late, `414` when the request line passes its cap, `431` when the head passes 16 KiB, `400` for a bare LF. On the complete head: the request line (`400`, or `414` for the target), its HTTP version (`505`), the header lines (`431` past 64, `400`). Then `Host` (`400`, `421`), the method (`405`), the body (`400`), the route (`404`), the query (`400`), and the core.

## Headers

Every API response, and every error, carries exactly these (a web file's headers are [above](#the-web-page---web-root)):

- `Content-Type: application/json`
- `Cache-Control: no-store`
- `X-Content-Type-Options: nosniff`
- `Referrer-Policy: no-referrer`
- `X-Frame-Options: DENY`
- `Content-Security-Policy: default-src 'none'; frame-ancestors 'none'`
- `Connection: close`
- `Content-Length: <bytes>` (except on a response to a `HEAD` request, and on the accept loop's `503` past both connection caps: neither has a body or this header)

plus `Allow: GET` on a `405` (`Allow: GET, HEAD` on a web path), and `Retry-After: 5` on a `503`. There is no CORS header (`Access-Control-*`), no `Set-Cookie`, no `Server`, no `ETag` and no `Last-Modified`.

### `Host` (DNS-rebinding defence)

A request must carry exactly one `Host`, and it must be one of (case-insensitive):

- `127.0.0.1:<port>`, `localhost:<port>` or `[::1]:<port>`, with this server's port;
- the one configured public host name, exactly, without a port.

The public host name comes only from local configuration: `--allowed-host NAME`, or the environment variable `OPENWIZ_SERVER_ALLOWED_HOST`. It is never committed. It must be a DNS name (letters, digits, `-` and `.`). Any other `Host` is `421`, before the method or route is looked at, and nothing is read. This adds a restriction and grants nothing: it is not authentication. Access control is the tunnel's and Access's (see `docs/web-access.md`).

## Bounds

| Bound | Value |
| --- | --- |
| Connections handled at once | 32 connections. One more gets `503` and `Retry-After: 5` from a separate, equally capped set of threads (with the `503` body, or none for `HEAD`). Beyond both caps, or if a thread cannot be started, the accept loop answers at once and without waiting: `503`, `Retry-After: 5` and the usual headers, but **no body and no `Content-Length`** (it does not wait to learn the method), then a half-close and a non-blocking drain of what the client already sent. Never closed unanswered |
| Core runs at once | 2 core runs (`list_report` or `show_report`). Each `ls -l` run already reads up to 4 sessions at once inside the core |
| Wait for a run | 5 seconds for a core run slot, then `503` with `Retry-After: 5`. Waiting requests are bounded by the connection cap: no queue grows |
| Identical requests | Requests for the same call (the same route and the same parsed options) while a run for it is in progress share that run and its result (single-flight). A request after the run ends starts a new run: there is no result cache, so every body has its own snapshot time and row ages |
| Request deadline | 40 seconds from the end of the request head, above the core's 30-second run budget plus the slot wait. A run past it keeps its slot until the core returns |
| Request head | 10 seconds for the whole request head (not per read), then `408` |
| Response | 10 seconds for the whole response, then the connection is closed |
| Request target | 1024 bytes |
| Request head size | 16 KiB, request line and headers |
| Header lines | 64 header lines |
| Web page (`--web-root`) | 1 MiB per file, 512 files and 8 MiB in all, loaded at start; past any cap, the server does not start |

## Log

One line per request, on stderr: `openwiz-server: <method> <route> <status> <n>ms`. The method is a standard method name or `-`; the route is a route template (such as `/api/v1/sessions/{id}`, or a web route: `/`, `/_next/static/*`, `/fonts/*`) or `-`. Never the query, the id, a header value or any session content.

## Not in version 1

- Server-side validation of the Cloudflare Access token: the server relies on the tunnel and Access in front of it, and on its `Host` check.
- Any cache, compression, keep-alive, CORS, cookie or write.
