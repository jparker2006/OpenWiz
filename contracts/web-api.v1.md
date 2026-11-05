# OpenWiz web API, version 1

*Updated for 0.0.5.* This document specifies fixture-tested behaviour; it does not certify live acceptance.

The HTTP API of `openwiz-server` (`crates/server`), for OpenWiz 0.0.2, 0.0.3 and 0.0.4. It is a thin client of the core, like the `openwiz` CLI: its inventory and session routes call the same core operation with the same options, and each of their success bodies is the `openwiz.inventory.v1` envelope that `openwiz … --json` prints. `/api/v1/health` is the exception: the server's own metadata, with no CLI equivalent and no `room-cli` read. The envelope's schema is [`contracts/inventory.v1.schema.json`](inventory.v1.schema.json), reused unchanged. Nothing here adds a field, an operation or a rule.

**Revision 0.0.3: from read-only to one mutation.** 0.0.2's API had no mutation surface (A4). This revision adds exactly one: `POST /api/v1/sessions/{id}/messages`, the core's `send`, as `openwiz send … --json`; and two reads that never send: the send preview and the send lookup (the core's `preview` and `lookup`, as `openwiz send … --preview --json` and `… --check --json`). Their bodies are the CLI's `openwiz.messaging.v1`, `openwiz.send-preview.v1` and `openwiz.send-lookup.v1` JSON ([`contracts/messaging.v1.schema.json`](messaging.v1.schema.json)), reused unchanged. See [Sending](#sending-the-one-write).

**Revision 0.0.4: a second mutation, new threads.** This revision adds exactly one more write: `POST /api/v1/projects/{id}/threads`, the core's `new_thread`, as `openwiz new --project-id <id> … --yes --json`; one read that never creates but carries the prompt, so it is a `POST`: the new-thread preview, the core's `preview_new_thread`, as the same command without `--yes`; and two reads: the Cloud options (`openwiz models --json`) and the new-thread status (`openwiz new --status <op_id> --json`). Their bodies are the CLI's `openwiz.new-thread.v1`, `openwiz.new-thread-preview.v1`, `openwiz.cloud-options.v1` and `openwiz.new-thread-status.v1` JSON ([`contracts/new-thread.v1.schema.json`](new-thread.v1.schema.json)), reused unchanged. **There is no retry route** (the request contract). See [Creating](#creating-the-second-write).

**Revision 0.0.4: attachments.** This revision adds the attachment routes: `POST /api/v1/attachments`, which stages one file the owner chose in the browser, as `openwiz attachment add <file> --json` stages one file; `GET /api/v1/attachments`, as `openwiz attachment ls --json`; and `DELETE /api/v1/attachments/{id}`, as `openwiz attachment rm <id> --json`. Their bodies are the CLI's `openwiz.attachment-add.v1`, `openwiz.attachment-list.v1` and `openwiz.attachment-remove.v1` JSON ([`contracts/attachment.v1.schema.json`](attachment.v1.schema.json)), reused unchanged. The send body gains the optional `attachments` (absent means none), and the create's and the new-thread preview's `attachments` now carry staged ids: the CLI's `--attachment <id>`, in order. **No route returns an attachment's bytes.** The upload and the removal change only the controller's attachment store, never CloudRoom. See [Attachments](#attachments-revision-004), and the [attachment limits](../docs/attachment-types.md).

**Status: Current interface; fixture test.** The current send, create and attachment routes are covered by synthetic tests. The 0.0.3 PC test held its release-blocking claims with gaps; the 0.0.4 run recorded held claims and gaps, including missing observed spawn-receipt fixture coverage; see [testing and gaps](../docs/testing.md).

## Transport

- **Loopback only.** The server listens on `127.0.0.1:<port>` and nothing else; no option chooses another address. The port is `--port N`, default `7370` (provisional); `--port 0` picks a free one. Nothing opens an inbound port.
- **HTTP/1.1 and HTTP/1.0 over plain TCP.** One request per connection: every response carries `Connection: close`. No keep-alive, no pipelining, no `Upgrade` (a WebSocket request gets an ordinary response), no server-sent events, no compression.
- **`GET` only** on the API, except four **`POST`** routes: the send ([Sending](#sending-the-one-write)), the create and the new-thread preview ([Creating](#creating-the-second-write)), and the upload ([Attachments](#attachments-revision-004)); one **`DELETE`**, the removal of a staged attachment; and **`GET` and `HEAD`** on the web page's paths when `--web-root` is given ([below](#the-web-page---web-root)). No request body is accepted, except a `POST`'s: JSON, or an upload's raw bytes.
- **Same origin.** No CORS header is ever sent, so a browser page on another origin cannot read a response.

## Routes

| Route | Same as | Success body |
| --- | --- | --- |
| `GET /api/v1/inventory` | `openwiz ls` or `ls -l`, with the query below | The envelope, as `ls --json` or `ls -l --json` prints it |
| `GET /api/v1/sessions/{id}` | `openwiz show <id> --json` | The envelope, as `show --json` prints it |
| `POST /api/v1/sessions/{id}/messages` | `openwiz send <id> --op-id <op> [--steer] [--resume-paused-queue] [--accept-possible-duplicate] --json -- <text>` | The send report (`openwiz.messaging.v1`), as `send --json` prints it. **The one write.** Only on the configured host name; see [Sending](#sending-the-one-write) |
| `GET /api/v1/sessions/{id}/preview` | `openwiz send <id> --preview [--steer] --json` | The preview (`openwiz.send-preview.v1`). Reads the target; never claims or sends |
| `GET /api/v1/sessions/{id}/messages/{op_id}` | `openwiz send <id> --op-id <op_id> --check --json` | The lookup (`openwiz.send-lookup.v1`). Reads the send record only; no `room-cli` call; never claims or sends |
| `GET /api/v1/cloud-options` | `openwiz models --json` | The Cloud options (`openwiz.cloud-options.v1`). One `cloud status` read; never creates |
| `POST /api/v1/projects/{id}/threads/preview` | `openwiz new --project-id <id> --harness <h> --model <m> --effort <e> --op-id <op> --json -- <text>` (without `--yes`) | The new-thread preview (`openwiz.new-thread-preview.v1`): the effects before pressing. Reads only; **never claims, records or creates**. A `POST` only because it carries the prompt; see [Creating](#creating-the-second-write) |
| `POST /api/v1/projects/{id}/threads` | `openwiz new --project-id <id> --harness <h> --model <m> --effort <e> --op-id <op> --yes --json -- <text>` | The new-thread report (`openwiz.new-thread.v1`), as `new --yes --json` prints it, with the start read once. **The second write.** Only on the configured host name |
| `GET /api/v1/new-threads/{op_id}` | `openwiz new --status <op_id> --json` | The status (`openwiz.new-thread-status.v1`). Reads the record and what it names; **never claims, writes or creates** |
| `POST /api/v1/attachments` | `openwiz attachment add <file> --json`, for one file | The add report (`openwiz.attachment-add.v1`) of that one file: ready or failed, with the core's reason. **Raw body** (`application/octet-stream`), never multipart. Only on the configured host name; see [Attachments](#attachments-revision-004) |
| `GET /api/v1/attachments` | `openwiz attachment ls --json` | The list report (`openwiz.attachment-list.v1`): the staged attachments' records and states. **Never their bytes**; no `room-cli` call |
| `DELETE /api/v1/attachments/{id}` | `openwiz attachment rm <id> --json` | The remove report (`openwiz.attachment-remove.v1`). Refused while an operation may still use it. Only on the configured host name |
| `GET /api/v1/health` | Nothing: no `room-cli` read | `{"server": {"name": "openwiz-server", "version": "<crate version>"}, "source": {"name": "room-cli", "tested_version": "<the version last live-tested: evidence, not a gate>"}}` |

Paths match exactly, case-sensitive, with no trailing slash and no percent-decoding. `{id}` and `{op_id}` are each one path segment. Any other path is `404` (`no_route`), except the web page's files when `--web-root` is given; so is `/api/v1/new-threads/{op_id}/retry`, for every method (there is no retry, the request contract). On `/api/v1/sessions/{id}/messages`, `/api/v1/projects/{id}/threads` and `/api/v1/projects/{id}/threads/preview`, any method other than `POST` is `405` with `Allow: POST`. On `/api/v1/attachments`, any method other than `GET` and `POST` is `405` with `Allow: GET, POST`; on `/api/v1/attachments/{id}`, any method other than `DELETE` is `405` with `Allow: DELETE`. Any method other than `GET`, on any other path, is `405` with `Allow: GET`. That includes `HEAD`; only the web page's paths take it. As HTTP requires, a response to a request that is, or may be, a `HEAD` request has the usual headers but **no body and no `Content-Length`** (which would have to describe a `GET`'s body: RFC 9110, 8.6 and 9.3.2), whatever its status: a web file's `200`, a `404` or `405`, an earlier refusal (`400`, `408`, `414`, `421`, `431`, `505`), or the connection cap's `503`. "May be" means its first bytes, as far as they arrived, are `HEAD ` or a non-empty beginning of it (`H`, `HE`, `HEA`, `HEAD`): over the connection cap the server peeks at them for up to 200 ms; otherwise they are the bytes read before the refusal. A response to a request none of whose bytes arrived, or whose first bytes cannot begin `HEAD `, has its body and `Content-Length` as usual. The send and the create are the only writes to CloudRoom; the upload and the removal change only the controller's attachment store. There are no retry, launch, approval or other write routes, and no route that returns an attachment's bytes.

## The web page (`--web-root`)

`openwiz-server --web-root DIR` also serves `apps/web`'s static export (`npm run build` makes `apps/web/out`) on the same origin as the API: a same-origin static export. **Without `--web-root`, every non-API path is `404`**, as before.

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
  - `Content-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self'; font-src 'self'; img-src 'self' blob:; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'` . **`img-src blob:`**: the tray's thumbnails are object URLs of the owner's own chosen file, made in the browser; nothing else changes, and no server answer is ever an image;
  - `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer`, `X-Frame-Options: DENY`, `Connection: close` and `Content-Length`;
  - no CORS header, `Set-Cookie`, `Server`, `ETag` or `Last-Modified`.

  Every refusal on a web path (`400`, `404`, `405`, `421`, …) is the API's JSON error with the API's headers.
- **Caching** is the builder's default, for the owner to confirm: the hashed assets and fonts are cached by the owner's browser for a year (`private`: never by a shared cache such as Cloudflare's edge), and the page is never stored, so a new build is picked up on the next load. The fonts are not content-hashed: a changed font needs a new file name.

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

## Sending (the one write)

Added in revision 0.0.3. Owner identity is checked by the tunnel's Access check before a request reaches the server; the server never reads a `Cf-Access-*` header or the `CF_Authorization` cookie. Sends are **never coalesced**: the read routes' single-flight never applies to them, and two sends share nothing except through their op id.

### `POST /api/v1/sessions/{id}/messages`

**The target is `{id}`, and only `{id}`.** The request carries, all exactly once:

| Header | Value |
| --- | --- |
| `Host` | The configured public host name (`--allowed-host`), exactly. **Never loopback:** a send with a loopback `Host` is `421`, and without a configured host name every send is `421`. The host name itself can never be loopback or an IP address ([below](#host-dns-rebinding-defence)) |
| `Origin` | `https://<allowed-host>`: the scheme `https`, the configured host name (case-insensitive), no port, no path |
| `Sec-Fetch-Site` | `same-origin` |
| `X-OpenWiz-Send` | `1` |
| `Content-Type` | `application/json`, optionally followed by `; charset=utf-8` (case-insensitive, optional spaces around `;`); nothing else |
| `Content-Length` | The body's length in decimal digits. `Transfer-Encoding` is refused |

Browsers set `Origin` and `Sec-Fetch-Site` themselves; a page sends the request with `fetch(url, {method: "POST", headers: {"Content-Type": "application/json", "X-OpenWiz-Send": "1", "X-Requested-With": "XMLHttpRequest"}, body, credentials: "same-origin", redirect: "manual", cache: "no-store"})`. `X-Requested-With` is not checked by the server: Cloudflare Access needs it to answer an expired session with `401` instead of a redirect. No query: any `?` is `400` (`invalid_request`). `Expect` is ignored: no `100 Continue` is sent.

**The body** is one JSON object, UTF-8, at most **99 328 bytes**:

```json
{
  "op_id": "web-0123456789abcdef0123456789abcdef-q1",
  "mode": "queue",
  "text": "Synthetic message text",
  "resume_paused_queue": false,
  "accept_possible_duplicate": false
}
```

| Key | Required | Value | CLI |
| --- | --- | --- | --- |
| `op_id` | yes | A string, the core's op-id rule: 1 to 64 ASCII letters, digits, `_` or `-`, starting with a letter or digit. The web makes `web-`, 32 lowercase hex digits, `-`, then `q` or `s` and a number , once per draft and mode, and reuses it on every retry and reload. The server applies only the core's rule, never this form | `--op-id` |
| `mode` | yes | `"queue"` or `"steer"`: the mode of the button pressed | `--steer` for `"steer"` |
| `text` | yes | A string: the message, sent as it is. The core's text rule (not empty, not only whitespace, no NUL, at most 16 384 bytes of UTF-8) | the text after `--` |
| `resume_paused_queue` | no (absent is `false`) | `true` only from the owner's explicit choice to resume a paused queue | `--resume-paused-queue` |
| `accept_possible_duplicate` | no (absent is `false`) | `true` only from the owner's explicit choice to send knowing it may duplicate an earlier send | `--accept-possible-duplicate` |
| `attachments` | no  | A JSON array of strings: staged attachment ids, in the owner's order, through the core's one gate ([Attachments in a send and a create](#attachments-in-a-send-and-a-create)). Never with `"mode": "steer"` (the core refuses it) | `--attachment <id>`, once per id |

Any other key (including `target`), a repeated key, a value of the wrong JSON type, a root that is not an object, invalid JSON or UTF-8, or anything after the object but whitespace is `400` (`invalid_request`), and nothing is read, claimed or sent. The message names the rule, never the value. **Only this one shape is a body:** an array (even one that lists the five values in order), `null` or any other JSON form is refused; `op_id` and `text` are JSON strings, the two flags JSON `true` or `false`, `mode` only the JSON string `"queue"` or `"steer"` (never an object such as `{"queue": null}`), and `attachments` only an array of JSON strings. Keys may come in any order.

**The answer** is the CLI's, by its exit code:

| CLI exit | Status | Body |
| --- | --- | --- |
| 0, 3, 5, 6, 7, 8, 9 (queued or sent, not found, failed, uncertain, in progress, conflicting reuse, possible duplicate refused) | `200` | The send report, byte for byte what `openwiz send … --json` prints for the same input and clock. **The outcome is in the report (`status`, `result`), never in the HTTP status** |
| 2 (the core refused the request: the target id, the op id or the text) | `400` | `invalid_request`, with the core's message (it never repeats the text) |
| 10 (the send record could not be used) | `500` | `record_unavailable`: "the send record could not be used; nothing was sent" (no path, ever) |

A `504` (`deadline`) means the answer did not come within the request deadline: **the send may still happen** and its outcome is recorded; the web shows it as uncertain and checks with the lookup. The core run is never cancelled.

**Order of checks** (each refusal makes no `room-cli` call and claims nothing): the head (as for every request), `Host` syntax (`400`), `Host` allowed at all (`421`), the method (`405`), the route (`404`), the send `Host`: the configured host name (`421`), the query (`400`), `Origin` and `Sec-Fetch-Site` (`403`, `forbidden_origin`), `X-OpenWiz-Send` (`403`, `missing_custom_header`), `Content-Type` (`415`, `unsupported_media_type`), the body's framing (`Transfer-Encoding` or a repeated or malformed `Content-Length`: `400`, `bad_request`; no `Content-Length`: `411`, `length_required`; over the cap: `413`, `body_too_large`, without reading it), the body's arrival (`408`, `request_timeout`), the JSON (`400`, `invalid_request`), a send slot (`503`, `busy`), then the core's `send`.

### `GET /api/v1/sessions/{id}/preview`

What a send would do now, from one read of the target (`thread show`, its row in `thread list`, and for a Local thread `machine list`). No text, no op id, no claim, no `tell`. Query: `mode=queue` or `mode=steer`, optional (default `queue`), at most once; anything else is `400` (`invalid_request`). The usual `GET` rules apply (loopback or the configured host name; no `Origin` check: a page on another origin cannot read the answer).

| CLI exit | Status | Body |
| --- | --- | --- |
| 0, 3, 5 (ready, not found, a send would be refused or needs acknowledgement) | `200` | The preview, byte for byte what `openwiz send <id> --preview [--steer] --json` prints |
| 2 (the target id is refused) | `400` | `invalid_request`, with the core's message |

It is a core run like `show`: it takes a read slot, and identical previews (the same id and mode) in progress share one run.

### `GET /api/v1/sessions/{id}/messages/{op_id}`

The send record of `{op_id}` for `{id}`, as it is now. **No `room-cli` call; never claims, writes or sends.** Opens the send record read-only (a missing record directory is "no record"). The usual `GET` rules apply; no query.

| CLI exit | Status | Body |
| --- | --- | --- |
| 11 (no record), 8 (bound to another target), and every exit of a recorded report (0, 3, 5, 6, 7, 9) | `200` | The lookup, byte for byte what `openwiz send <id> --op-id <op_id> --check --json` prints |
| 2 (the target id or the op id is refused) | `400` | `invalid_request`, with the core's message |
| 10 | `500` | `record_unavailable`, as for the send |

"No record" proves only that no send with that op id was claimed in this send record: a request still on its way can claim it afterwards. Sending again with the **same** op id is always safe: it is never a second send.

### One send record

The server opens the same send record as `openwiz send`: `$OPENWIZ_STATE_DIR`, else the owner's default state directory (`openwiz-store`'s `default_state_dir`), read when each request needs it. Run it as the owner, with the same environment as the CLI, so that the CLI and the web share op-id claims (U9). The server never sets its own.

## Creating (the second write)

Added in revision 0.0.4. Owner identity is the tunnel's. Creates are **never coalesced**: the read routes' single-flight never applies to them, and two creates share nothing except through their op id. **There is no retry**: an uncertain create stays uncertain, and nothing here, in the core or in the web, ever creates a second time by itself.

The four routes call only the core, as `openwiz new` and `openwiz models` do, and answer with what the CLI prints with `--json`. The server checks only the request's shape (the headers, the body's framing, size and JSON, below) and works out no option, effect, outcome, stage or retry. **Without a new-thread configuration** (an in-process server in a test), all four are `404` for every method.

### `POST /api/v1/projects/{id}/threads`

**The project is `{id}`, and only `{id}`:** one existing project, by its id, passed to the core as `--project-id` would be. The request carries the send's headers, all exactly once, with the same values and rules ([the send's table](#post-apiv1sessionsidmessages)): `Host` (the configured host name, never loopback: `421`), `Origin: https://<allowed-host>`, `Sec-Fetch-Site: same-origin`, `X-OpenWiz-Send: 1` (the send header, kept for every `POST`), `Content-Type: application/json` (optionally `; charset=utf-8`) and `Content-Length`; `Transfer-Encoding` is refused. No query: any `?` is `400` (`invalid_request`). `Expect` is ignored.

**The body** is one JSON object, UTF-8, at most **99 328 bytes** (the send's cap: the core's text cap fully JSON-escaped, plus 1 KiB for the other keys):

```json
{
  "op_id": "web-0123456789abcdef0123456789abcdef-c1",
  "harness": "codex",
  "model": "syn-codex-model-01",
  "effort": "high",
  "text": "Synthetic prompt text",
  "attachments": []
}
```

| Key | Value | CLI |
| --- | --- | --- |
| `op_id` | A string, the core's op-id rule (1 to 64 ASCII letters, digits, `_` or `-`, starting with a letter or digit). The web makes `web-`, 32 lowercase hex digits, `-c`, then a number (`web-<32 hex>-c<n>`, 39 characters for attempt 1), once per draft attempt, stores it first, and reuses it on every repeat and reload. The server applies only the core's rule, never this form | `--op-id` |
| `harness` | A string: the catalogue's harness id, as offered by `GET /api/v1/cloud-options`. The core's setting rule (1 to 128 printable ASCII characters, no space, not starting with `-`); nothing is defaulted | `--harness` |
| `model` | A string, the same rule | `--model` |
| `effort` | A string, the same rule | `--effort` |
| `text` | A string: the prompt, passed as it is. The core's text rule (not empty, not only whitespace, no NUL, at most 16 384 bytes of UTF-8) | the text after `--` |
| `attachments` | A JSON array of strings: staged attachment ids, in the owner's order, through the core's one gate (; [Attachments in a send and a create](#attachments-in-a-send-and-a-create)). `[]` is a text-only prompt | `--attachment <id>`, once per id |

All six keys are required, each exactly once, each of its own JSON type; keys may come in any order. Any other key, a missing or repeated key, a value of the wrong JSON type (an `attachments` element included), a root that is not an object, invalid JSON or UTF-8, or anything after the object but whitespace is `400` (`invalid_request`), and nothing is read, claimed or created. The message names the rule, never the value.

**Then the core, in `openwiz new`'s order:** the op id, then the request (the text, the project and the three settings): a refusal is `400` (`invalid_request`) with the core's message (it never repeats the text); then a create slot (`503`, `busy`); then the core's `new_thread`: the claim, the reads, the options check, the effects, the one `spawn` and the outcome. For a **created** thread (recorded now, or replayed), the server then reads its start **once** (`thread show`), as the CLI's first follow read (the CLI follows it for up to 60 s; the web follows it with its own refresh, through the status route).

**The answer** is the CLI's, by its exit code:

| CLI exit | Status | Body |
| --- | --- | --- |
| 0, 5, 6, 7, 8 (created; refused or failed; uncertain; in progress; conflicting reuse) | `200` | The new-thread report, as `openwiz new … --yes --json` prints it for the same input and clock, with `start` from one read. **The outcome is in the report (`status`, `result`), never in the HTTP status** |
| 2 (the core refused the op id, the text, the project or a setting) | `400` | `invalid_request`, with the core's message |
| 10 (the new-thread record could not be used) | `500` | `record_unavailable`: "the new-thread record could not be used; nothing was created" (no path, ever) |

A `504` (`deadline`) means the answer did not come within the request deadline: **the create may still happen** and its outcome is recorded; the web shows it as uncertain and reads the status route. The core run is never cancelled. A repeat of the **same** op id and body, from a reload, a second tab or a client retry, returns the recorded outcome with no call (or "in progress" while the first runs); with any other project, setting or text it is conflicting reuse, with no call.

**Order of checks** (each refusal makes no `room-cli` call and claims nothing): the head (as for every request), `Host` syntax (`400`), `Host` allowed at all (`421`), the method (`405`, `Allow: POST`), the route (`404`), the create `Host`: the configured host name (`421`), the query (`400`), `Origin` and `Sec-Fetch-Site` (`403`, `forbidden_origin`), `X-OpenWiz-Send` (`403`, `missing_custom_header`), `Content-Type` (`415`, `unsupported_media_type`), the body's framing (`400`, `411`, or `413` without reading it), the body's arrival (`408`), the JSON (`400`), the core's op-id and request rules (`400`), a create slot (`503`), then the core's `new_thread`.

### `POST /api/v1/projects/{id}/threads/preview`

What the create would do now, from the reads it makes (the version, the project list, the catalogue): the core's `preview_new_thread`, as `openwiz new … --json` without `--yes`. **No claim, no record, no `spawn`.** It is a `POST` only because it carries the prompt (too long for a URL), and it takes **the create's headers, body and order of checks, unchanged**, so the launcher shows the core's effect lines before pressing (U9). It is a core run like `show`: it takes a read slot (`503` without one), and identical previews in progress (the same project and body) share one run.

| CLI exit | Status | Body |
| --- | --- | --- |
| 12 (ready: the effects, and the `spawn` provider), 5 (a create would be refused or fail, and why) | `200` | The preview, as `openwiz new … --json` prints it without `--yes` |
| 2 | `400` | `invalid_request`, with the core's message |

### `GET /api/v1/cloud-options`

The harnesses, models and efforts CloudRoom offers now, from one `cloud status` read with only the catalogue keys (the request contract): `openwiz models --json`. Nothing is marked default or preselected. No query (`400`). The usual `GET` rules apply (loopback or the configured host name). It is a core run: a read slot, shared by identical requests in progress.

| CLI exit | Status | Body |
| --- | --- | --- |
| 0 (something is offered), 13 (nothing can be offered: the body says why) | `200` | The options, as `openwiz models --json` prints them |

### `GET /api/v1/new-threads/{op_id}`

The record of `{op_id}` and, for a created thread, its start stage from one `thread show`; for an uncertain one, the threads seen in its project since (observations only: they never settle it). **Read-only: never claims, writes or creates, and never calls `spawn`.** Opens the new-thread record read-only (a missing record directory is "no record"). The usual `GET` rules apply; no query. It is a core run: a read slot, shared by identical requests in progress.

| CLI exit | Status | Body |
| --- | --- | --- |
| 11 (no record), and every exit of a recorded report (0, 5, 6, 7, 8) | `200` | The status, as `openwiz new --status <op_id> --json` prints it |
| 2 (the op id is refused) | `400` | `invalid_request`, with the core's message |
| 10 | `500` | `record_unavailable`, as for the create |

"No record" proves only that no create with that op id was claimed in this record: a request still on its way can claim it afterwards. Repeating the create with the **same** op id is always safe: it is never a second thread.

### One new-thread record

The server opens the same new-thread record as `openwiz new`: `new-threads/` in `$OPENWIZ_STATE_DIR`, else the owner's default state directory, read when each request needs it, so the CLI and the web share op-id claims. The server never sets its own.

## Attachments (revision 0.0.4)

Added in revision 0.0.4. Owner identity is the tunnel's. The three routes call only the attachment store (`openwiz-store`) and the core's rules, as `openwiz attachment add`, `ls` and `rm` do, and answer with what the CLI prints with `--json`; **none calls `room-cli`.** The server checks only the request's shape and bounds, and works out no kind, name, limit, floor, gate or retention rule. **Without an attachment configuration** (an in-process server in a test), all three are `404` for every method, and a send or create with attachments is refused by the core's gate (no attachment is found).

**No route returns an attachment's bytes**, ever: not the upload, the list, the removal, an error, or any other path. The web's thumbnails come from the browser's own copy of the chosen file.

### `POST /api/v1/attachments`

One file's bytes, staged as `openwiz attachment add <file> --json` stages one file: streamed straight into the attachment store's staging in 64 KiB pieces (no file is written anywhere else), hashed as it arrives, then verified. Its kind (from the bytes), safe name, provider limits (10 MiB per image, 25 MiB per file, the image-typed extensions), the free-disk floor and the extension rule are the core's, checked before the copy is written. **Ready means only that the controller holds a verified copy**: nothing is sent to CloudRoom.

The request carries, all exactly once:

| Header | Value |
| --- | --- |
| `Host` | The configured public host name, exactly; never loopback (`421`), as for the send |
| `Origin` | `https://<allowed-host>` |
| `Sec-Fetch-Site` | `same-origin` |
| `X-OpenWiz-Send` | `1` (the send header, kept for every write) |
| `Content-Type` | `application/octet-stream`, exactly (case-insensitive), with no parameter. **Never `multipart/form-data`** or any other type |
| `Content-Length` | The body's length in decimal digits: at most **26 214 400** (25 MiB, the core's per-file limit `FILE_MAX_BYTES`, the largest provider limit; the request rules). `Transfer-Encoding` is refused |
| `X-OpenWiz-Name` | The file's name, as the browser reports it, percent-encoded UTF-8 (what `encodeURIComponent` makes: only ASCII letters, digits, `-_.!~*'()` and `%XX`). Decoded, one file name: 1 to 1 024 bytes of UTF-8, with no `/` or NUL, and not `.` or `..` (as a name `openwiz attachment add` reads from a path always is). Untrusted text: the core makes the staged name from it, and it is never a path |

No query (`400`). `Expect` is ignored. A page sends it with `XMLHttpRequest` (for upload progress): `open("POST", "/api/v1/attachments")`, the headers above plus `X-Requested-With: XMLHttpRequest` (for Access's `401`), and the `File` as the body.

**The body** is the file's bytes, unchanged, exactly `Content-Length` of them. They are never read past `Content-Length`. A body that ends early, or doesn't arrive within the upload deadline, stages nothing and leaves nothing behind (no `.partial`).

**The answer** is the CLI's add report, for one file at index `0`, whose `path` is the decoded name (an upload has no path):

| CLI | Status | Body |
| --- | --- | --- |
| exit 0 (ready), 5 (failed: the core's reason) | `200` | The add report (`openwiz.attachment-add.v1`), as `openwiz attachment add <file> --json` prints it for the same bytes, name and clock: in `ready`, the staged record (id, SHA-256, size, kind, safe name, the original name) and its state; in `failed`, `readiness` with the core's `reason` and its message. `expired` lists what this upload's open of the store swept, as `add` does. **The outcome is in the report, never in the HTTP status** |
| exit 10 (the store, or the free-space measure, could not be used) | `500` | `store_unavailable`: "the attachment store could not be used; nothing was staged" (no path, ever) |

**Order of checks** (each refusal stages nothing): the head (as for every request), `Host` syntax (`400`), `Host` allowed at all (`421`), the method (`405`, `Allow: GET, POST`), the route (`404`), the upload `Host`: the configured host name (`421`), the query (`400`), `Origin` and `Sec-Fetch-Site` (`403`, `forbidden_origin`), `X-OpenWiz-Send` (`403`, `missing_custom_header`), `Content-Type` (`415`, `unsupported_media_type`), the body's framing (`Transfer-Encoding` or a repeated or malformed `Content-Length`: `400`, `bad_request`; none: `411`; over 26 214 400: `413`, without reading it), `X-OpenWiz-Name` (`400`, `invalid_request`), an upload slot (`503`, `busy`), then the store, which checks the core's rules on the first bytes and the declared size before writing anything, and the body's arrival (`408`, `request_timeout`, past the upload deadline; a body that ends early gets no answer, since the client has gone). After a refusal, the server reads and discards what the client still sends, up to `Content-Length`, within the deadline, so a browser can read the answer.

### `GET /api/v1/attachments`

`openwiz attachment ls --json`: every staged attachment's record and state, oldest first, then the broken ones, and what this read's open swept (unfinished copies, and attachments past their 24-hour time-to-live, the request rules), as `ls` does. **Never the bytes, and never a path.** No query (`400`). The usual `GET` rules apply (loopback or the configured host name; no `Origin` check: another origin can't read the answer). No `room-cli` call.

| CLI | Status | Body |
| --- | --- | --- |
| exit 0 | `200` | The list report (`openwiz.attachment-list.v1`), as `openwiz attachment ls --json` prints it. Nothing ever staged is the empty list, and creates nothing |
| exit 10 | `500` | `store_unavailable` |

### `DELETE /api/v1/attachments/{id}`

`openwiz attachment rm <id> --json`: deletes one staged copy and its record, unless an operation is sending it or may have sent it (the request rules: refused, and kept). `{id}` is one path segment, the CLI's rule (an attachment id: 1 to 64 ASCII letters, digits, `_` or `-`, starting with a letter or digit). The request carries the upload's `Host`, `Origin`, `Sec-Fetch-Site` and `X-OpenWiz-Send`, exactly once each, and **no body** (no `Content-Length` but `0`, no `Transfer-Encoding`: `400`, `bad_request`). No `Content-Type` is needed. No query (`400`).

| CLI | Status | Body |
| --- | --- | --- |
| exit 0, 3 (not found), 14 (refused) | `200` | The remove report (`openwiz.attachment-remove.v1`), as `openwiz attachment rm <id> --json` prints it |
| exit 2 (not an attachment id) | `400` | `invalid_request`, with the CLI's message |
| exit 10 (the store failed on it) | `500` | `store_unavailable`: "the attachment store could not be used; nothing was removed" |

**Order of checks:** the head, `Host` (`400`, `421`), the method (`405`, `Allow: DELETE`), the route (`404`), the removal `Host`: the configured host name (`421`), the query (`400`), `Origin` and `Sec-Fetch-Site` (`403`), `X-OpenWiz-Send` (`403`), the body (`400`), the id (`400`), then the store.

### Attachments in a send and a create

- **The send body** gains one optional key, `attachments`: a JSON array of strings, each a staged attachment's id, in the owner's order; absent means none (0.0.3's bodies are unchanged). The CLI's `--attachment <id>`, once per id, in order.
- **The create and the new-thread preview bodies**: `attachments` stays required, and now carries staged ids the same way (`[]` is a text-only prompt).
- Each id must be a JSON string; anything else is `400` (`invalid_request`, the body rule). The ids pass **the core's rules and its one gate**, exactly as the CLI's: an id it refuses, a repeated id, Steer with attachments, or any attachment not ready, expired or in use is `400` (`invalid_request`) with the core's words, naming each one; **nothing is claimed, read or sent**, and no `room-cli` call is made. The server checks none of it itself. The answers are the CLI's, with the attachments the core reports.
- The body caps are unchanged (99 328 bytes): the text and the ids share them.

### Bounds

| Bound | Value |
| --- | --- |
| Upload body | 26 214 400 bytes (25 MiB, the request rules), with `Content-Length`; streamed in 64 KiB pieces, never held whole |
| Uploads at once | **2**, apart from the sends, creates and core runs; an upload waits up to 5 s for a slot, then `503`. Taken after the head's checks, before the body is read |
| Upload deadline | **110 seconds** from the end of the request head, for the whole body and its verification (the request rules: under Cloudflare's 125 s proxy read timeout, documented, not observed); then `408`, and nothing is staged |
| List and removal | No body; no slot of their own (local disk only) |

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

The server's own errors (every error that is not a core outcome, including a bad query's `invalid_request`) never repeat the request's values and carry no file path, environment, stack trace or credential. A core outcome's message is the CLI's: `not_found`, and the core's `invalid_request` for a refused id, can name the id asked for, and a failed read's message carries `room-cli`'s first non-empty stderr line only if that line is in OpenWiz 0.0.1's safe grammar (`Error: HTTP <code>`, optionally `: ` and a reason phrase of ASCII letters and single spaces, at most 60 characters). Otherwise a line containing `HTTP 404: Thread not found` becomes `Error: HTTP 404: Thread not found (detail withheld)` (and a detail read reports that session as not found); any other line keeps only its status, as `Error: HTTP <code> (detail withheld)`, or nothing of it, as `room-cli error (detail withheld)`; an empty stderr is `no error output`. Text `room-cli` prints where a version, JSON or a recorded value is expected is withheld the same way. The same rule holds for a success body's envelope, in `gaps` and in `unknown (…)` reasons, as in `openwiz … --json`. The rule is the adapter's , not the server's; the server adds to the core's text only what the CLI adds: the source's fixed read name (`thread list`, `thread show`) before a failed read. The `room-cli` version is never an error: any version is read, shown in the envelope's `source`, and the run goes on. A letters-only word in a safe line (a name, say) can still be shown, and session content (titles, purposes, final output, a recorded error) is data, shown by design.

### Every status

| Status | `error.kind` | When |
| --- | --- | --- |
| `200` | (none) | A result, or the health body; and every send report, preview and lookup, every new-thread report, preview, status and options body, and every attachment add, list and remove report, whatever its outcome |
| `400` | `bad_request` | A malformed request line or header, CRLF missing, a leading blank line, a missing or repeated `Host`, a malformed `Host`, any request body (`Content-Length` above 0, or any `Transfer-Encoding`) except a `POST` route's; on a `POST` route (the send, the create, the new-thread preview, the upload): any `Transfer-Encoding`, or a repeated or malformed `Content-Length` |
| `400` | `invalid_request` | A malformed, unknown or repeated query parameter; a query on a route that takes none; the core's `InvalidRequest` (the gate's refusal of an attachment included); a send or create body that breaks its body rules; an upload's missing, repeated or malformed `X-OpenWiz-Name`; a removal's id that is not an attachment id |
| `403` | `forbidden_origin` | A `POST` (send, create, new-thread preview or upload) or the `DELETE` whose `Origin` is missing, repeated or not `https://<allowed-host>`, or whose `Sec-Fetch-Site` is missing, repeated or not `same-origin` |
| `403` | `missing_custom_header` | A `POST` or the `DELETE` without exactly one `X-OpenWiz-Send: 1` |
| `404` | `no_route` | Any path other than the routes above (`/api/v1/new-threads/{op_id}/retry` included), or, with `--web-root`, a web path that is not served |
| `404` | `not_found` | `show` of a session the source does not know |
| `405` | `method_not_allowed` | Any method other than `GET`; with `Allow: GET`. On `/api/v1/sessions/{id}/messages`, `/api/v1/projects/{id}/threads` and `/api/v1/projects/{id}/threads/preview`, any method other than `POST`; with `Allow: POST`. On `/api/v1/attachments`, any method other than `GET` and `POST`; with `Allow: GET, POST`. On `/api/v1/attachments/{id}`, any method other than `DELETE`; with `Allow: DELETE`. On a web path, any method other than `GET` and `HEAD`; with `Allow: GET, HEAD` |
| `408` | `request_timeout` | The request head, or a `POST`'s body, did not arrive within its time (an upload's: the upload deadline) |
| `411` | `length_required` | A `POST` without `Content-Length` |
| `413` | `body_too_large` | A `POST` whose `Content-Length` is over 99 328 bytes, or an upload's over 26 214 400 (the body is not read) |
| `415` | `unsupported_media_type` | A `POST` whose `Content-Type` is missing, repeated, or not `application/json` (optionally `; charset=utf-8`); an upload's, not exactly `application/octet-stream` |
| `414` | `target_too_long` | The request target is longer than its cap |
| `421` | `misdirected` | The `Host` is not this server's loopback address or the configured host name; or a `POST`'s `Host` (send, create or new-thread preview) is not the configured host name (always, when none is configured) |
| `431` | `head_too_large` | The request head, or its number of header lines, is over its cap |
| `500` | `internal` | A core run failed unexpectedly, or could not be started. Not expected |
| `500` | `record_unavailable` | A send or lookup whose send record could not be used, or a create or status read whose new-thread record could not be used (the CLI's exit 10). Nothing was sent or created |
| `500` | `store_unavailable` | The attachment store (or its free-space measure) could not be used for an upload, a list, a removal, or a send or create with attachments. Nothing was staged, removed, sent or created |
| `502` | `read_failed` | As above |
| `503` | `busy` | Over the connection cap, or no core run slot (or, for a send, no send slot; for a create, no create slot; for an upload, no upload slot) within its wait; with `Retry-After: 5`. A send, create or upload refused so is not claimed, sent, created or staged |
| `504` | `deadline` | No result within the request deadline (only if a source ignores the core's budget). For a send or a create: it may still happen and is recorded; the client treats it as uncertain |
| `505` | `http_version` | An HTTP version other than 1.1 and 1.0 |

The checks run in this order. While the head arrives: `408` when it is late, `414` when the request line passes its cap, `431` when the head passes 16 KiB, `400` for a bare LF. On the complete head: the request line (`400`, or `414` for the target), its HTTP version (`505`), the header lines (`431` past 64, `400`). Then `Host` (`400`, `421`), the method (`405`), the body (`400`, except on a `POST` route), the route (`404`), the query (`400`), and the core. The send route's own order is [in its section](#post-apiv1sessionsidmessages), and the create's and the new-thread preview's [in theirs](#post-apiv1projectsidthreads).

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

plus `Allow: GET` on a `405` (`Allow: POST` on the send, create and new-thread preview routes, `Allow: GET, POST` on `/api/v1/attachments`, `Allow: DELETE` on `/api/v1/attachments/{id}`, `Allow: GET, HEAD` on a web path), and `Retry-After: 5` on a `503`. There is no CORS header (`Access-Control-*`), no `Set-Cookie`, no `Server`, no `ETag` and no `Last-Modified`.

### `Host` (DNS-rebinding defence)

A request must carry exactly one `Host`, and it must be one of (case-insensitive):

- `127.0.0.1:<port>`, `localhost:<port>` or `[::1]:<port>`, with this server's port;
- the one configured public host name, exactly, without a port.

The public host name comes only from local configuration: `--allowed-host NAME`, or the environment variable `OPENWIZ_SERVER_ALLOWED_HOST`. It is never committed. It must be a DNS name (letters, digits, `-` and `.`), and, since it is also the send's host name, **a public one**: not a loopback name (`localhost`, any `*.localhost`, `localhost.localdomain`, `ip6-localhost`, `ip6-loopback`, in any case) and not an IP address (a last label of digits only, as in `127.0.0.1`, `127.1` or `2130706433`, or starting `0x`). Otherwise `openwiz-server` does not start (exit 2), and nothing is read. Any other `Host` is `421`, before the method or route is looked at, and nothing is read. **A send, a create and a new-thread preview accept only the configured host name**: with a loopback `Host` each is `421`, after the route is matched, so a page served from loopback can never send or create. This adds a restriction and grants nothing: it is not authentication. Access control is the tunnel's and Access's (see `docs/web-access.md`).

## Bounds

| Bound | Value |
| --- | --- |
| Connections handled at once | 32 connections. One more gets `503` and `Retry-After: 5` from a separate, equally capped set of threads (with the `503` body, or none for `HEAD`). Beyond both caps, or if a thread cannot be started, the accept loop answers at once and without waiting: `503`, `Retry-After: 5` and the usual headers, but **no body and no `Content-Length`** (it does not wait to learn the method), then a half-close and a non-blocking drain of what the client already sent. Never closed unanswered |
| Core runs at once | 2 core runs (`list_report`, `show_report`, the send `preview`, or since 0.0.4 `cloud_options`, `preview_new_thread` or `new_thread_status`). Each `ls -l` run already reads up to 4 sessions at once inside the core |
| Wait for a run | 5 seconds for a core run slot, then `503` with `Retry-After: 5`. Waiting requests are bounded by the connection cap: no queue grows |
| Identical requests | Requests for the same call (the same route and the same parsed options) while a run for it is in progress share that run and its result (single-flight). A request after the run ends starts a new run: there is no result cache, so every body has its own snapshot time and row ages |
| Sends at once | 2 sends (the core's `send`), apart from the core runs; a send waits up to 5 seconds for a slot, then `503`. **Never single-flighted:** every accepted send is its own call of `send`; only the op id joins two requests |
| Creates at once | **1 create** (the core's `new_thread`, the request contract), apart from the sends and the core runs; a create waits up to 5 seconds for the slot, then `503`. **Never single-flighted:** every accepted create is its own call of `new_thread`; only the op id joins two requests |
| Send and create body | 99 328 bytes (`6 × 16 384 + 1 024`: the core's text cap fully JSON-escaped, plus the other keys), with `Content-Length`, for each `POST` (the send, the create, the new-thread preview); it must arrive within 10 seconds of the end of the head |
| Request deadline | 40 seconds from the end of the request head, above the core's 30-second run budget plus the slot wait. A run past it keeps its slot until the core returns; a send or a create past it still finishes and records its outcome |
| Request head | 10 seconds for the whole request head (not per read), then `408` |
| Response | 10 seconds for the whole response, then the connection is closed |
| Request target | 1024 bytes |
| Request head size | 16 KiB, request line and headers |
| Header lines | 64 header lines |
| Web page (`--web-root`) | 1 MiB per file, 512 files and 8 MiB in all, loaded at start; past any cap, the server does not start |

## Log

One line per request, on stderr: `openwiz-server: <method> <route> <status> <n>ms`. The method is a standard method name or `-`; the route is a route template (such as `/api/v1/sessions/{id}`, `/api/v1/sessions/{id}/messages`, `/api/v1/sessions/{id}/preview`, `/api/v1/sessions/{id}/messages/{op_id}`, `/api/v1/cloud-options`, `/api/v1/projects/{id}/threads`, `/api/v1/projects/{id}/threads/preview` `/api/v1/new-threads/{op_id}`, `/api/v1/attachments` or `/api/v1/attachments/{id}`, or a web route: `/`, `/_next/static/*`, `/fonts/*`) or `-`. Never the query, the id, the project id, the op id, an attachment id, a header value, a body, a message or prompt text or its hash, a setting, a file's name, bytes, size, kind or hash, or any session content.

## Not in version 1

- Server-side validation of the Cloudflare Access token. See the access evaluation.
- Any cache, compression, keep-alive, CORS or cookie; any write other than the send and the create, and the upload and removal of a staged attachment.
- More than one target per send, more than one project or thread per create, a retry of a create (the request contract), a Local thread, a new project, or a title or branch for a new thread.
- Any route that returns an attachment's bytes; multipart uploads; chunked or resumable uploads; folders; Steer with attachments.
