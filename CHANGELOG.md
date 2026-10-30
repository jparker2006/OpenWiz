# Changelog

All OpenWiz versions and revisions, newest first. Versioning is explained in [RELEASING.md](RELEASING.md). PACP has its own version history in [docs/pacp/v0.1.md](docs/pacp/v0.1.md#version-history).

## 0.0.1 · 2026-10-30

**See your CloudRoom threads from the terminal**

1. `openwiz ls` lists your threads with their status, project and how fresh the data is; `-a` and `--hidden` add archived and hidden ones.
2. `openwiz ls -l` adds a one-line recap and the reported model and effort.
3. `openwiz show <id>` shows one thread's details and its final output.
4. `--json` on `ls` and `show`, for scripts.
5. Read-only: it never starts, messages or changes a thread, and in a live check on macOS with `room-cli` v127 no idle thread was woken or marked read.
6. Runs with any `room-cli` version and never refuses an update; versions other than v127 are shown as untested.

What was tested, and what is not proven: [docs/evidence/0.0.1-live-check.md](docs/evidence/0.0.1-live-check.md). Dependencies: [docs/dependencies.md](docs/dependencies.md).

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
