# Dependencies of OpenWiz 0.0.2

**Status: Current** for the code in this repository. *Updated for 0.0.2:* the server adds no crate, and the web page's npm packages are [below](#the-web-pages-packages). Every third-party crate in `Cargo.lock` is listed here with its exact version, licence and checksum. `cargo test -p openwiz-tests --test checks` fails if a locked crate is missing from this table or its version or checksum differs.

OpenWiz depends directly on two crates, `serde` and `serde_json`. Any other direct dependency needs the maintainer's approval. The other crates come in through those two. Proc-macro crates run only while building, and are not part of the program.

| Crate | Version | Licence | Checksum (SHA-256, from `Cargo.lock`) | Why it is here |
| --- | --- | --- | --- | --- |
| `serde` | `1.0.229` | MIT OR Apache-2.0 | `4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba` | Direct: serialization, with the `derive` feature |
| `serde_json` | `1.0.154` | MIT OR Apache-2.0 | `e7e9cc8b1b85264074fbcc02a88680c4096b1e47df8f739dceb03bf482f04bd6` | Direct: reads `room-cli` JSON and prints `--json` |
| `serde_core` | `1.0.229` | MIT OR Apache-2.0 | `67dca2c9c51e58a4791a4b1ed58308b39c64224d349a935ab5039aa360942a48` | Through `serde` and `serde_json` |
| `serde_derive` | `1.0.229` | MIT OR Apache-2.0 | `e7a5d71263a5a7d47b41f6b3f06ba276f10cc18b0931f1799f710578e2309348` | Through `serde` (proc macro, build only) |
| `proc-macro2` | `1.0.107` | MIT OR Apache-2.0 | `985e7ec9bb745e6ce6535b544d84d6cd6f7ad8bd711c398938ae983b91a766d9` | Through `serde_derive` (build only) |
| `quote` | `1.0.47` | MIT OR Apache-2.0 | `1fbf4db142a473a8d80c26bbf18454ed458bf8d26c8219c331daecfdbd079001` | Through `serde_derive` (build only) |
| `syn` | `3.0.9` | MIT OR Apache-2.0 | `d78c8dee4c7bf0e14673097256fed6142ce9d3b85a408189d07482442145823b` | Through `serde_derive` (build only) |
| `unicode-ident` | `1.0.27` | (MIT OR Apache-2.0) AND Unicode-3.0 | `a2c754d6c33795a1c324727428e5a7dedb5b06195f9890bdbcba760d3e246563` | Through `proc-macro2` and `syn` (build only) |
| `itoa` | `1.0.18` | MIT OR Apache-2.0 | `8f42a60cbdf9a97f5d2305f08a87dc4e09308d1276d28c869c684d7777685682` | Through `serde_json` |
| `memchr` | `2.8.3` | Unlicense OR MIT | `cf8baf1c55e62ffcace7a9f06f4bd9cd3f0c4beb022d3b367256b91b87513d98` | Through `serde_json` |
| `zmij` | `1.0.23` | MIT | `29666d0abbfad1e3dc4dcf6144730dd3a3ab225bbbdac83319345b1b44ccfc1b` | Through `serde_json` |

The licences are as each crate declares them in its package metadata. This repository does not include any crate's source; `cargo` downloads them from crates.io and checks each against the checksum above.

## The web page's packages

*New in 0.0.2.* `apps/web/package-lock.json` pins every package with its exact version and an integrity hash, and `npm ci` checks each one. Install with `npm ci --ignore-scripts`: no package needs an install script. Any new package needs the maintainer's approval.

**Direct dependencies.** The built page holds only their browser code, with that of the packages they bring in:

| Package | Version | Licence | Why |
| --- | --- | --- | --- |
| `next` | `16.4.3` | MIT | The framework: a static export (`output: 'export'`, Pages Router), built with its bundled webpack |
| `react` | `19.3.1` | MIT | The page's components |
| `react-dom` | `19.3.1` | MIT | Renders them in the browser |

**Build and test tools**, never in the built page:

| Package | Version | Licence | Why |
| --- | --- | --- | --- |
| `typescript` | `6.0.3` | Apache-2.0 | Type checking (`npm run typecheck`) and the static checks (`npm run check`) |
| `@types/node` | `24.19.2` | MIT | Types for Node.js |
| `@types/react`, `@types/react-dom` | `19.3.0` | MIT | Types for React |
| `playwright-core` | `1.64.1` | Apache-2.0 | Drives Chromium in the browser tests. Downloads no browser |
| `axe-core` | `4.14.0` | MPL-2.0 | Accessibility checks in the browser tests |

**Through those** (from the lockfile): `@next/env` 16.4.3 (MIT), `@swc/helpers` 0.5.23 (Apache-2.0), `baseline-browser-mapping` 2.11.33 (Apache-2.0), `caniuse-lite` 1.0.30001819 (CC-BY-4.0), `client-only` 0.0.1 (MIT), `csstype` 3.2.3 (MIT), `nanoid` 3.3.20 (MIT), `picocolors` 1.1.1 (ISC), `postcss` 8.5.23 (MIT), `scheduler` 0.28.0 (MIT), `source-map-js` 1.2.2 (BSD-3-Clause), `styled-jsx` 5.1.6 (MIT), `tslib` 2.8.1 (0BSD) and `undici-types` 7.24.6 (MIT).

**Optional, one per platform**, installed only where they match: Next's compiler (`@next/swc-*` 16.4.3, MIT), and the image library `sharp` 0.35.5 (Apache-2.0) with its `@img/sharp-*` builds (Apache-2.0; the `libvips` builds are LGPL-3.0-or-later), `@img/colour` 1.1.0 (MIT), `detect-libc` 2.1.2 (Apache-2.0), `semver` 7.8.5 (ISC) and `@emnapi/runtime` 1.11.3 (MIT). The page uses no image optimisation, so `sharp` does nothing in its build.

The licences are as each package declares them in the lockfile. The repository includes no package's code.

**Fonts.** Geist and Geist Mono (variable, `woff2`), bundled in `apps/web/public/fonts/` and served by the page itself, never from another site. They are licensed under the SIL Open Font License 1.1, whose text is next to them in `OFL.txt`.

## Tools

| Tool | Version | Status |
| --- | --- | --- |
| Rust (`rustc`, `cargo`) | 1.99.1, the `rust-version` in `Cargo.toml` | Required to build. Older versions are not supported |
| `rustfmt`, `clippy` | as shipped with Rust 1.99.1 | Needed only for the formatting and lint checks |
| `room-cli` | v127 tested; any version runs | Needed at run time. Other versions run and are shown as untested. See [what was tested](evidence/0.0.1-live-check.md) |
| Node.js, npm | 24.22.1 (the `engines` in `apps/web/package.json`), with npm 11.19.1 | *New in 0.0.2.* Needed only to build and test the web page; the server does not run Node.js |
| Chromium | 156 | *New in 0.0.2.* Needed only for the page's browser tests (`/usr/bin/chromium`, or `OPENWIZ_CHROMIUM`) |
| `cloudflared` | any current version | *New in 0.0.2.* Needed only to reach the page from outside your Mac ([setup](web-access.md)). Cloudflare Zero Trust, with Access, is needed for the same |

## Systems and protocols

| What | Status |
| --- | --- |
| macOS | Built natively and live-checked once (Apple Silicon). *Updated for 0.0.2:* the server too, behind Cloudflare Tunnel and Access ([evidence](evidence/0.0.2-live-check.md)) |
| Linux | Fixture tests pass, the web page's included |
| Windows | Untested |
| Browsers | *New in 0.0.2.* Chromium only, in the tests. Safari, iOS and others are untested |
| PACP | None: 0.0.2 implements no PACP version |
