# Dependencies of OpenWiz 0.0.1

**Status: Current** for the code in this repository. Every third-party crate in `Cargo.lock` is listed here with its exact version, licence and checksum. `cargo test -p openwiz-tests --test checks` fails if a locked crate is missing from this table or its version or checksum differs.

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

## Tools

| Tool | Version | Status |
| --- | --- | --- |
| Rust (`rustc`, `cargo`) | 1.99.1, the `rust-version` in `Cargo.toml` | Required to build. Older versions are not supported |
| `rustfmt`, `clippy` | as shipped with Rust 1.99.1 | Needed only for the formatting and lint checks |
| `room-cli` | v127 tested; any version runs | Needed at run time. See the [changelog](../CHANGELOG.md) for what was tested |
