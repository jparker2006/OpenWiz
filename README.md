# OpenWiz

*Updated for 0.0.5.* **Preview: message a thread, start a Cloud thread with attachments, and coordinate work with eight fleet skills.** This single update combines 0.0.3, 0.0.4 and 0.0.5.

**Fixture test:** the CLI and web client implement Queue and Steer messages, new-thread previews and creation in an existing project, and local attachment staging. A provider receipt is a report of acceptance or queueing, never proof that work completed. An uncertain result stays uncertain; the client does not silently repeat a create.

**LIVE-0.0.3 — live observation, passed with gaps:** the PC test held its release-blocking claims for correct targeting, lost acknowledgements, no redispatch, operation ids and owner-only checks. The recorded test spans earlier 0.0.3 candidates, ending on `d4b2a82`; it is not a live run of this combined export. Busy-Cloud Queue after the receipt fix was not re-observed; network-cut emulation did not produce the fault. Browser, access-path and identity gaps remain in [testing and gaps](docs/testing.md). **LIVE-0.0.4 — pending live test:** new-thread and attachment acceptance is running; no pass is claimed.

**Live observation, limited — 0.0.5:** the eight skills were used during a real development run and accepted with gaps. Review reports named exact commits and four merges matched approved heads. Skill discovery in fresh children was not observed; briefs used file paths. Five-ticket simultaneous launch, cross-version unblock timing and contract-change holds were not demonstrated live. Some starts, reviews and cleanup needed manual intervention. Fixture checks cover the helper scripts; this does not establish unattended operation or compatibility with every harness. See [testing and gaps](docs/testing.md).

## Install

**Current build requirements:** Rust 1.99.1 with rustfmt and clippy, Node.js 24.22.1, npm, zsh for the shell proofs, and Chromium for browser tests. This code targets Unix systems; the current export check uses Linux. Exact locked crates, packages and licences are in [dependencies](docs/dependencies.md).

```sh
cargo build --release --locked
cd apps/web
npm ci --ignore-scripts
npm run build
cd ../..
./target/release/openwiz --help
./target/release/openwiz-server --help
```

**Current interface:** install and sign in to CloudRoom separately. This release finds `room-cli` on `PATH`. Running inventory, model-catalogue and preview commands reads the real account. `openwiz send` can send a real message; `openwiz new --yes` can create a real Cloud thread. Attaching files to either uploads them through CloudRoom. Use the help text to inspect arguments before using a real account.

```sh
./target/release/openwiz-server --web-root apps/web/out
```

**Fixture test:** the server defaults to `127.0.0.1:7370`. It has write routes and no account login of its own. Remote access requires an owner-only access layer; configure that before exposing it. The [access guide](docs/web-access.md) and [API contract](contracts/web-api.v1.md) describe the boundaries. No new tunnel or account was exercised for this export.

## Fleet skills

**Current files:** [skills/README.md](skills/README.md) lists `grill`, `plan-graph`, `launch-fleet`, `build-ticket`, `review-ticket`, `handoff`, `setup` and `triage`. They are instructions and helper scripts, with a plugin manifest and repository aliases. Run `setup` in your project first and record your own approvals, agents and commands. Brief an agent with `Read skills/<name>/SKILL.md and follow it` when automatic discovery is unavailable.

## Check this tree

**Fixture test procedure:** build the web export before Rust or browser tests. All automated product tests use synthetic inputs and a fake `room-cli`.

```sh
cd apps/web
npm run typecheck
npm run check
cd ../..
cargo fmt --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cd apps/web
npm test
```

The private export record holds this head's actual command results. The public [testing page](docs/testing.md) distinguishes the automated check from live acceptance. There is no PACP conformance claim.

## What 0.0.1 does

**Historical live observation:** 0.0.1 listed and inspected CloudRoom sessions in a bounded macOS check. The 0.0.2 web viewer was also checked on macOS and visited on the owner's phone. Those older observations do not prove this update's live behaviour. Published reports remain available in the [0.0.2 public tree](https://github.com/jparker2006/OpenWiz/tree/54a96ddddddc80b41ce129485fa31fc2b67da5bf/docs/evidence).

## Status words

**Current** describes files or an interface present here. **Fixture test** describes synthetic automated checks. **Live observation** describes a bounded observed run. **Independent review** applies only to the exact head reviewed. **Unverified** and **pending live test** make no success claim. **Proposed** and **Future** describe design, not shipped behaviour. **Released** means published after maintainer approval; this staged update awaits that approval.

## Design and licence

The [architecture](docs/architecture.md), [workflow](docs/workflow.md) and [PACP draft](docs/pacp/v0.1.md) retain their own proposal labels. [Apache 2.0](LICENSE) covers OpenWiz; the skills retain Matt Pocock's MIT notices and the fonts retain their OFL notice.
