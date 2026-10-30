# Changelog

All OpenWiz versions and revisions, newest first. Versioning is explained in [RELEASING.md](RELEASING.md). PACP has its own version history in [docs/pacp/v0.1.md](docs/pacp/v0.1.md#version-history).

## 0.0.1 — PUBLICATION-DATE

The first version with software. **One outcome:**

1. **`openwiz ls`, `ls -l`, `show` and `--json`: a read-only inventory of your real CloudRoom sessions,** read through your existing, signed-in `room-cli` (checked live once, with v127: [evidence](docs/evidence/0.0.1-live-check.md)). Each result says whether it is complete or partial, when it was read, and what is missing. Unknown values stay unknown, and a session is grouped under a project only when that project was seen in the same run. See the [README](README.md#what-001-does).

### What was tested

| Evidence | What it covered | Result |
| --- | --- | --- |
| **Fixture tests** (in this repository) | The 12 proof cases P1 to P12 (coverage, freshness, unknowns, partial failures, paging and the time budget, verified grouping, bounded output, the read-only allowlist, reported configuration, JSON matching the text, visibility, version and schema drift), plus checks on dependencies, fixtures, limits and the JSON schema. They run against a fake `room-cli` with synthetic data | 91 proof tests and 16 checks pass on Linux with Rust 1.99.1. Anyone can rerun them with `cargo test --workspace` |
| **A native build on macOS** | `cargo build --release` on an Apple Silicon Mac with Rust 1.99.1 | Built and ran. The fixture tests last ran there on an earlier 0.0.1 build |
| **One live check, on one Mac, with `room-cli` v127** (2026-10-29): [method, rules and counts](docs/evidence/0.0.1-live-check.md) | The maintainer ran `ls`, `ls -l`, `show`, a missing-id `show` and a full paged listing against their real sessions (2,488), then compared every session's status, update time and read and attention times before and after, with a 120-second wait. For 20 sampled sessions it also compared their events and queued messages | **Passed:** no session that was quiet before the check changed; no sample gained an event or a queued message; P1, P4 and P6 were consistent; all 46 OpenWiz runs exited as expected, and none of its 305 reads was refused. One session that was already busy changed; that is reported, not judged. A first run was incomplete, not failed: the check's own comparison could not place 3 rows, so the check was corrected and run again |

The live check's raw records hold private session data, so only its method, rules, environment, counts and limits are published, in [docs/evidence/0.0.1-live-check.md](docs/evidence/0.0.1-live-check.md). The counts come from the check's own report; the session data behind them is not public.

The live check ran a build from just before the tested-version label was set to `v127`. Apart from comments and the crate version number, that label is the only difference in the program's code: the older build marked v127 as untested. A rerun of the check with the new label is planned; the [evidence page](docs/evidence/0.0.1-live-check.md#the-environment) says the same.

### Dependencies

| What | Version | Note |
| --- | --- | --- |
| `room-cli` (CloudRoom's command-line tool) | v127 | The only version live-checked. Any other version runs, and is shown as untested |
| Rust | 1.99.1 | The minimum (`rust-version`), and the only version tested |
| Crates | `serde` 1.0.229, `serde_json` 1.0.154, and 9 crates they use | All locked in `Cargo.lock`; listed with licences and checksums in [docs/dependencies.md](docs/dependencies.md) |
| Operating system | macOS (live check and build), Linux (fixture tests) | Windows is untested |
| PACP | None | 0.0.1 implements no PACP version |

### Not proven

- **That a read can never wake a session or mark it read.** The [live check](docs/evidence/0.0.1-live-check.md) found no such effect, but it had limits:
  - it did not inspect the requests `room-cli` sends;
  - it compared events and queued messages for 20 sessions only, and only the fields named above for the rest;
  - it waited 120 seconds, so slower effects would not show;
  - it did not judge sessions that were already busy, and hidden sessions were not checked separately;
  - its own reads were assumed to have no effect.
- **Freshness and unknowns against CloudRoom's own view.** These are fixture-tested only; no live comparison was made.
- **Slow machines and other output.** Whether `thread list` always finishes within the 15-second read limit on a slower machine, whether `room-cli` ever prints anything other than JSON (such as an update notice), and whether it starts helper processes, are all unobserved.
- **Anything beyond CloudRoom.** No other service, personal agent or PACP behaviour is part of 0.0.1.

The text output is provisional and may change. The `--json` format is versioned as `openwiz.inventory.v1`.

### Documents updated for 0.0.1

The 0.0.0 design documents are unchanged except where 0.0.1 needed to say something new; each such sentence is marked *Updated for 0.0.1*. The README is rewritten around 0.0.1, with the design text kept below it.

## 0.0.0 — revision 1 (unreleased, under review)

Still a design preview with no implementation. Four outcomes:

1. **Renamed to OpenWiz, with its technology and layout decided.**
   - The core, the CLI and all backend logic are Rust. The web client is Next.js and TypeScript. Desktop packaging is undecided.
   - One monorepo, with dependencies pointing inward. Clients keep presentation state only; adapters keep integration bookkeeping only and never own policy.
   - CLI-first and a polished UI are both goals.
   - **The controller is the one Rust core that owns authoritative state, permissions, routing and coordination.** Each project orchestrator runs its factory workflow under it and is not a second authority.
2. **How OpenWiz is used and how it works:**
   - [docs/interface.md](docs/interface.md): commands, decision types, honest lists, screens and visual style.
   - [docs/workflow.md](docs/workflow.md): who owns what, ticket states (with Define proof before Build), rework, pause and resume, approval, dependent unlock and a worked example. Correction rounds are counted per review finding: two failed rounds, a diagnosis that is not a round, one more round, then the user decides. Thresholds are configurable, with defaults 2 and 1.
   - Service access that exists today is kept separate from OpenWiz adapters, none of which is implemented.
3. **Two architecture images**, each with an editable SVG source and a 3840×2160 PNG. One shows the product architecture, the other the monorepo modules. Both are labelled as proposals, with dashed lines for unverified links.
4. **Release rules:**
   - proof-driven development (claims and their evidence are defined before building);
   - version lines separated from real dependencies;
   - at most five outcomes per version;
   - this changelog;
   - the planned scope of 0.0.1, recorded as not started: a read-only Rust inventory of real CloudRoom sessions.

PACP gets an editorial note on the rename only. It stays v0.1 (draft).

## 0.0.0 — 2026-10-22

First publication, as OpenFleet: a design preview with no implementation.
- Overview, architecture diagrams and decisions.
- PACP v0.1 (draft).
- Contribution and release rules.
- Apache License 2.0.
