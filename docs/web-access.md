# Reaching OpenWiz from your phone: Cloudflare Tunnel and Access

**New in 0.0.2.** How to open your own `openwiz-server` from your own phone or laptop, from any network, with only you let in. It uses a Cloudflare Tunnel (no open port on your Mac) and Cloudflare Access (a login in front of it).

**Placeholders only.** Replace each `<…>` with your own value, and never commit those values anywhere:

| Placeholder | What it is |
| --- | --- |
| `<your-domain>` | A domain in your Cloudflare account |
| `<openwiz-hostname>` | The one hostname you will open, such as `openwiz.<your-domain>` |
| `<team-name>` | Your Cloudflare Zero Trust team name |
| `<your-email>` | The email address you sign in with |
| `<aud-tag>` | The Access application's audience tag |
| `<tunnel-name>`, `<tunnel-id>` | The tunnel's name, and the id Cloudflare gives it |
| `<port>` | The server's local port (default `7370`) |
| `<web-dir>` | A copy of the web page's build, `apps/web/out` |

**Status:** this setup was used for 0.0.2's live check, on one Mac. What was checked, and what was not, is in [evidence/0.0.2-live-check.md](evidence/0.0.2-live-check.md). Check your own installation as in [Check it](#check-it) before you use it with real data.

## How it is protected

- **Your Mac, your login.** The server reads your threads through your own signed-in `room-cli`. Nothing is shared between installations.
- **Loopback only.** `openwiz-server` listens on `127.0.0.1` and nowhere else. Nothing else on your network can reach it, and you open no inbound port. The tunnel connects outward from your Mac to Cloudflare.
- **Read-only.** The server answers `GET` on three API routes, plus the page's own files. There is no route that writes, starts or messages anything ([the contract](../contracts/web-api.v1.md)).
- **Only you get in.** Cloudflare Access sits in front of `<openwiz-hostname>` and lets in only the emails in its policy. The tunnel itself also refuses a request without a valid Access token for this application.
- **Foreign hosts are refused.** The server answers only requests for its loopback address or for `<openwiz-hostname>`, which stops a web page on another site from using your browser to read it (DNS rebinding). This check is not a login; Access is.
- **What Cloudflare sees.** HTTPS ends at Cloudflare's edge, so your thread titles, purposes and final output pass through Cloudflare unencrypted. Decide whether you accept that before you set this up.
- **What any program on your Mac can do.** Like `openwiz` and `room-cli` themselves, any process on your Mac can call the loopback server. A web page open in a browser on your Mac can send it requests but cannot read the answers (no CORS header).
- **Not for this:** Cloudflare's Quick Tunnels (`trycloudflare.com`). Their hostname is random and changes, and nothing like Access sits in front of them.

## What you need

- A Mac with `room-cli` installed and signed in, awake while you want to look.
- A Cloudflare account with `<your-domain>` on it (the Free plan is enough).
- Cloudflare Zero Trust, for Access. Its Free plan covered up to 50 users when this was written; check Cloudflare's current pricing.
- `cloudflared`, Cloudflare's connector, on the Mac.
- To build: Rust 1.99.1, and Node.js 24.22.1 with npm (on any machine; only the built files go to the Mac).

## Steps

Do each account step yourself. Keep every credential on your Mac or in Cloudflare.

### 1. Build the server and the page

On the Mac, from a clone of this repository:

```sh
cargo build --release -p openwiz-server
```

The web page can be built on the Mac or anywhere else with Node.js 24.22.1:

```sh
cd apps/web
npm ci --ignore-scripts
npm run build          # writes apps/web/out
```

Copy `apps/web/out` to `<web-dir>` on the Mac if you built it elsewhere. To be sure the copy is complete, compare `find . -type f | LC_ALL=C sort | xargs shasum -a 256`, run inside both folders.

The server loads `<web-dir>` once, when it starts, and serves it from memory, so **restart the server after replacing it**. It refuses to start if anything in `<web-dir>` is a symlink, a hard link, a hidden file (such as macOS's `.DS_Store`: delete it) or a file type the page does not use, and its message names the rule that failed.

### 2. Try it with test data first

The repository has a fake `room-cli` that serves invented threads only. Run the server against it before connecting anything real:

```sh
cargo build -p openwiz-tests --bin fake-room-cli
mkdir -p <scratch-dir>
ln -s "$PWD/target/debug/fake-room-cli" <scratch-dir>/room-cli
echo "$PWD/tests/fixtures/room-cli-0.43.1/normal/scenario.json" > <scratch-dir>/fake-room-cli.scenario
cd <scratch-dir>
PATH="$PWD:/usr/bin:/bin" <repo>/target/release/openwiz-server --port <port> --allowed-host <openwiz-hostname> --web-root <web-dir>
```

The fake looks for its scenario file next to its own link, or in the folder it runs in, which is why it gets a folder of its own and the server starts there. Every answer then has only `thr_syn_` ids and "Synthetic" titles. Check it locally:

```sh
curl -H 'Host: 127.0.0.1:<port>' http://127.0.0.1:<port>/api/v1/health
curl -sI -H 'Host: 127.0.0.1:<port>' http://127.0.0.1:<port>/        # 200, text/html
```

For real data, start the server the same way but without the fake on `PATH`, so it runs your own `room-cli`:

```sh
target/release/openwiz-server --port <port> --allowed-host <openwiz-hostname> --web-root <web-dir>
```

Instead of `--allowed-host`, you can set `OPENWIZ_SERVER_ALLOWED_HOST=<openwiz-hostname>`. The server prints `listening on http://127.0.0.1:<port> (loopback only, with the web page)`.

### 3. Set up Access

1. In Cloudflare Zero Trust, create your team (`<team-name>`; its sign-in domain is `<team-name>.cloudflareaccess.com`) and pick a login method: Cloudflare's one-time PIN by email is enough, or a sign-in provider you already use.
2. Add an Access **self-hosted application** for `<openwiz-hostname>`, covering all paths.
3. Give it exactly **one Allow policy**, whose only rule includes `<your-email>` (or several emails, all yours). Add no Bypass and no Service Auth rule.
4. Choose how long a login lasts (24 hours is Cloudflare's default).
5. Turn on the `401` response for expired requests made in the background, so the page can say "signed out" instead of "disconnected".
6. Note the application's audience tag, `<aud-tag>`.

### 4. Create the tunnel

Following Cloudflare's guide to [a locally-managed tunnel](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/do-more-with-tunnels/local-management/create-local-tunnel/):

```sh
cloudflared tunnel login                                        # in your browser, pick <your-domain>
cloudflared tunnel create <tunnel-name>                         # prints <tunnel-id> and writes its credentials file
cloudflared tunnel route dns <tunnel-name> <openwiz-hostname>   # points the hostname at the tunnel
```

Write a configuration file for this tunnel only, for example `~/.cloudflared/openwiz.yml`. It sends `<openwiz-hostname>` to the loopback server, requires an Access token at your end, and answers everything else with `404`:

```yaml
tunnel: <tunnel-id>
credentials-file: <home>/.cloudflared/<tunnel-id>.json
ingress:
  - hostname: <openwiz-hostname>
    service: http://127.0.0.1:<port>
    originRequest:
      access:
        required: true
        teamName: <team-name>
        audTag:
          - <aud-tag>
  - service: http_status:404
```

Route nothing else through this tunnel. Check the file, then start the tunnel while the server runs:

```sh
cloudflared tunnel --config ~/.cloudflared/openwiz.yml ingress validate
cloudflared tunnel --config ~/.cloudflared/openwiz.yml run <tunnel-name>
```

The login certificate and the tunnel's credentials file stay on the Mac. Never put them in a repository, an issue or a chat.

### 5. Keep it running, or not

Both the server and `cloudflared` stop when you close their terminals or restart the Mac. If you want them always on, run them as login items or `launchd` agents, and decide whether the Mac may sleep. A sleeping Mac answers nothing.

## Check it

Before using it with real data:

- **Loopback only.** `lsof -nP -iTCP:<port> -sTCP:LISTEN` shows one listener, on `127.0.0.1:<port>`. From another device on your network, `http://<mac-lan-address>:<port>/` is refused.
- **No login, no data.** From another network, with no Access session, every path of `https://<openwiz-hostname>/` (try `/` and `/api/v1/inventory`) gives Cloudflare's login page or a redirect to it, never JSON. The same with a made-up `CF_Authorization` cookie.
- **Not you, no data.** Signing in with an email outside the policy is refused. If you have no second email to try, at least check in Zero Trust that the application has the one Allow policy, with only your emails.
- **You get in.** Signed in on your phone, the page shows your threads, and the header says how fresh they are.

## What you will see

- **Signed in, Mac awake:** your threads, refreshed while the page is open.
- **Login expired:** Cloudflare's login page when you load the page; "signed out" on a page that is already open.
- **Mac asleep, or the server or tunnel stopped:** Cloudflare's error page when you load the page; "disconnected", with the time of the last good data, on a page that is already open.
