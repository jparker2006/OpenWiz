# Contributing

OpenWiz is a **design preview**. *Updated for 0.0.1:* 0.0.1 adds one small, read-only command, `openwiz`; everything else is still a proposal, with no code. The most useful contribution right now is critical feedback.

## What helps

Open an issue about:

- a term or boundary that is unclear or wrong;
- a failure mode the design or PACP does not handle;
- an existing standard or protocol we should reuse instead of inventing something;
- a claim that goes further than the evidence.

## Pull requests

OpenWiz is licensed under the [Apache License 2.0](LICENSE). Unless you state otherwise, any contribution you submit is licensed under the same terms, as described in section 5 of the license. There is no separate contributor agreement.

For anything beyond a small fix, please open an issue first so the change can be discussed while this is still a proposal.

*Updated for 0.0.1:* a code change must pass, with Rust 1.99.1:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The tests use a fake `room-cli` and synthetic fixtures only; `cargo test -p openwiz-tests --test checks fixture_scan` refuses anything that looks real. A new third-party crate needs the maintainer's approval and a row in [docs/dependencies.md](docs/dependencies.md). The tests never call your real `room-cli`.

## Rules for anything you post

1. **Synthetic examples only.** Use reserved names such as `example.com`, `example.org`, `urn:example:…` and `did:example:…`, and invented people such as "User A".
2. **No personal data, secrets or private material.** That includes real names, contact details, transcripts, chat logs, memory exports, tokens, keys, credentials, private links, and session or account identifiers.
3. **Label status honestly.** Use **Released**, **Current**, **Unverified**, **Proposed** or **Future** as defined in the [README](README.md#status-words) (*Released* is new in 0.0.1). Do not describe anything as working, supported, compatible or secure without evidence anyone can check.
4. **Do not imply access that does not exist.** Many personal agents have no public interface for this kind of integration. Say "requires a supported interface", not "integrates with".
5. **Attribute sources.** Link public sources, pinned to an exact version or commit where possible. Quote briefly rather than copying. Only submit material you have the right to license under Apache 2.0.

If you post something sensitive by mistake, say so in the issue, and the maintainer will remove it.
