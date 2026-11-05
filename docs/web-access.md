# Private access to OpenWiz

*Updated for 0.0.5.* **Current interface; fixture-tested boundaries.** The current server can send messages, create Cloud threads and stage or remove local attachments. The 0.0.3 PC test held its release-blocking claims with recorded gaps; LIVE-0.0.4 remains **pending live test**. See [testing and gaps](testing.md). The old read-only 0.0.2 access check does not validate this update's writes.

## How it is protected

The server binds to `127.0.0.1` only and has no account login of its own. It uses the signed-in `room-cli` on `PATH`. Serve the page and API on the same origin. Remote access requires a separate owner-only authentication layer covering every path, before any request reaches the server. A tunnel alone is insufficient.

**Fixture test:** configure one public DNS hostname with `--allowed-host <openwiz-hostname>` or `OPENWIZ_SERVER_ALLOWED_HOST`. The server checks Host, Origin and request headers as the [API contract](../contracts/web-api.v1.md) specifies. Send, creation and new-thread preview refuse loopback Host; their host check grants no identity. Upload and removal also require the configured host. A local process is not a protected trust boundary.

## Build and run

Use the [README build steps](../README.md#install), then keep the complete `apps/web/out` together with the corresponding server build. The server loads that directory at startup; restart after replacing it.

```sh
target/release/openwiz-server --port <port> --allowed-host <openwiz-hostname> --web-root <web-dir>
```

All `<…>` values are local placeholders. Store account credentials and tunnel configuration outside Git. The server and access connector must remain running. The machine running them must remain awake while it serves requests; closing a separate viewing device is not the same as stopping that host.

## Check it

**Unverified for your installation:** configure the access layer using its current vendor instructions. If using Cloudflare Tunnel and Access, require validation of the Access token at the origin as well as an owner-only policy. HTTPS terminates at the provider's edge, so thread data and submitted prompts/files pass through that provider.

Before connecting a real account, exercise the fixture tests. Before using a remote endpoint with real data, check without a login, with a made-up session cookie, and with an identity outside the allowed policy: every route must stop at the access layer. Check that only the owner can reach the page and that the server still binds only to loopback. Viewing a page or seeing an accepted receipt is not proof that a requested write completed.

**Attachments:** staging copies selected bytes into the local attachment store; a ready copy has not been delivered. A Queue send or create uploads it through room-cli. Removal is local and does not prove deletion of CloudRoom's copy. Steer cannot carry attachments. The [attachment table](attachment-types.md), [API](../contracts/web-api.v1.md) and current `openwiz --help` define limits and commands.

**Historical live observation:** the older access path was checked for 0.0.2 on macOS. Its published report remains in the [0.0.2 public tree](https://github.com/jparker2006/OpenWiz/blob/54a96ddddddc80b41ce129485fa31fc2b67da5bf/docs/evidence/0.0.2-live-check.md). No new tunnel, real-account read or write, Safari/WebKit session or physical mobile-radio test was performed by this export preparation. See [testing and open placeholders](testing.md).
