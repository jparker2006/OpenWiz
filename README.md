# OpenWiz

> [!IMPORTANT]
> **0.0.2: your CloudRoom threads in the terminal and on your phone, read-only. Everything else here is still a proposal.**
> 0.0.1 added `openwiz`, a command that lists your CloudRoom sessions through `room-cli`. 0.0.2 adds `openwiz-server`, which shows the same list as a private web page from your own Mac, reached through a Cloudflare Tunnel and an Access login. Both only read. Each was checked live on one Mac: see [0.0.1's check](docs/evidence/0.0.1-live-check.md) and [0.0.2's](docs/evidence/0.0.2-live-check.md). The controller, the agent commands, the rest of OpenWiz Web, every other adapter and the PACP protocol are design documents, with no implementation.

## What 0.0.2 adds

**Status: Released.** What it adds is in the [changelog](CHANGELOG.md); what was tested, and what is not proven, is in [docs/evidence/0.0.2-live-check.md](docs/evidence/0.0.2-live-check.md).

`openwiz-server` runs on your Mac, next to your signed-in `room-cli`, and serves two things on `127.0.0.1` only:

- **A small read-only HTTP API:** `GET /api/v1/inventory` (the same as `openwiz ls` or `ls -l`), `GET /api/v1/sessions/<id>` (the same as `openwiz show`) and `GET /api/v1/health`. Each answer is exactly what `openwiz … --json` prints. The routes, statuses, headers and bounds are in [contracts/web-api.v1.md](contracts/web-api.v1.md).
- **The web page** (`apps/web`, with `--web-root`): your threads grouped by project, each with its status as a word and a symbol, its age, and a details sheet on tap (a side panel on a wide screen). It refreshes itself while open and says when the data is stale, partial or disconnected, or when you are signed out. It is a static Next.js export, served from the same origin as the API.

To reach it from your phone, put it behind a Cloudflare Tunnel and an Access login for your own email: [docs/web-access.md](docs/web-access.md) walks through it, with placeholders for your own values. The server never listens on your network, and you open no port.

**It only reads,** like `openwiz`: the server calls the same core and makes the same allowlisted `room-cli` reads. It has no route that writes, starts or messages anything, and it reads no file while answering.

### Run it

You need what `openwiz` needs (below), and Node.js 24.22.1 with npm to build the page:

```sh
cargo build --release -p openwiz-server
(cd apps/web && npm ci --ignore-scripts && npm run build)        # writes apps/web/out
./target/release/openwiz-server --web-root apps/web/out             # http://127.0.0.1:7370/
```

`openwiz-server --help` prints every option. Opening the page reads your real sessions through your signed-in `room-cli`, like `openwiz ls`. To try it on invented data first, see [step 2 of the setup guide](docs/web-access.md#2-try-it-with-test-data-first).

To check the build yourself:

```sh
cargo test --workspace                       # after building the page: A8 serves apps/web/out
(cd apps/web && npm run typecheck && npm run check && npm test)   # the page's proofs; need cargo and Chromium (/usr/bin/chromium, or set OPENWIZ_CHROMIUM)
```

### Limits of 0.0.2

- **Checked live once, on one Mac,** with a server built just before an unused part was removed from the shared code ([details](docs/evidence/0.0.2-live-check.md#the-environment)). Requests without a login got only the login page. That a second person is refused is known from Access's configuration only.
- **Chromium only.** The page's tests, including the keyboard and screen-reader checks, ran in Chromium. Safari, iOS and real screen readers are untested.
- **Your Mac must be awake,** with the server and the tunnel running. Nothing starts them for you.
- **Cloudflare sees your data.** HTTPS ends at Cloudflare's edge, so thread titles and output pass through Cloudflare.
- **Not proven:** the [list](docs/evidence/0.0.2-live-check.md#not-proven), including whether the page's own reads were part of the live check that saw no thread woken.

## What 0.0.1 does

**Status: Released.** What it adds is in the [changelog](CHANGELOG.md); what was tested, and what is not proven, is in [docs/evidence/0.0.1-live-check.md](docs/evidence/0.0.1-live-check.md). The only live evidence is [one check on one Mac](docs/evidence/0.0.1-live-check.md), with `room-cli` v127; everything below that it did not cover is fixture-tested only. *Updated for 0.0.2:* the same check was [rerun on 0.0.1's final build](docs/evidence/0.0.2-live-check.md#001s-live-check-rerun-on-its-final-build), and passed.

`openwiz` is a read-only inventory of your CloudRoom sessions, which CloudRoom calls threads. It reads them through `room-cli` (checked live with v127: [evidence](docs/evidence/0.0.1-live-check.md)), the CloudRoom command-line tool, which must already be installed and signed in on your machine.

| Command | What it prints |
| --- | --- |
| `openwiz ls` | Your sessions, newest first, grouped under `/factories/<project>/`. Each row has the session id, status, age, where it runs, its parent and its title |
| `openwiz ls -l` | The same, plus each session's provider and its purpose as reported, and its **reported** model and effort. For model and effort only, it reads each session on the page once more, with `thread show` |
| `openwiz show <id>` | One session in detail, with its final output (at most 4 KiB). Never a log |
| `--json` | The same result as JSON, in the `openwiz.inventory.v1` format ([schema](contracts/inventory.v1.schema.json)) |

`ls` also takes `-a` (include archived sessions), `--hidden` (include hidden sessions), `--limit N` (default 100, at most 200) and `--offset N`. `openwiz --help` prints the full usage and the exit codes.

**It only reads.** `openwiz` runs `room-cli` with six read commands and nothing else: `--version`, `thread list`, `project list`, `machine list`, `thread show <id>` and `thread output <id>`, each with only the flags in [its allowlist](crates/adapters/cloudroom/src/allowlist.rs). It never asks CloudRoom to start, message, archive or change a session. It writes no files and opens no network connection of its own; `room-cli` does the reading, and everything printed comes from that one run. In the [live check](docs/evidence/0.0.1-live-check.md), OpenWiz made 305 reads and none was outside this allowlist, and no quiet session changed in the compared fields within 120 seconds. That is not proof that a read can never wake a session or mark it read: see [Limits](#limits).

**It says what it does not know:**
- Every result says whether it is **complete or partial**, when it was read, and each known **gap**, such as a read that failed or a field that was missing.
- A value that could not be read is printed as `unknown (<reason>)`. Nothing is guessed.
- A session is grouped under a project only if that project is in the project list read in the same run. Anything else is shown under `/factories/_ungrouped/`, with the reason.
- Model and effort are labelled as reported by CloudRoom. They are not proof of the model that actually served a session.
- The `room-cli` version is shown on every run. 0.0.1 was [live-checked](docs/evidence/0.0.1-live-check.md) with `room-cli` v127. Any other version still runs, and is marked untested.
- Each read stops after 15 seconds and is never retried, and a whole run stops after 30 seconds. What did not fit is reported as a gap.

### Install

You need Rust 1.99.1 (the tested version; older ones are refused), and `room-cli`, installed and signed in. From a clone of this repository:

```sh
cargo build --release
./target/release/openwiz --help
```

The program is `target/release/openwiz`; copy it anywhere on your `PATH` if you like. There is no package or installer.

To check the build yourself:

```sh
cargo test --workspace     # about 40 seconds; uses a fake room-cli and synthetic data only
```

### Example

Every id, name and time below is invented, from the test fixtures:

```text
$ openwiz ls
openwiz ls · room-cli v127 (tested) · snapshot 2026-01-15T09:00:00Z · complete
counts: listed 6, unreadable 0, excluded_hidden 0, excluded_archived 0, hidden_read false, archived_read false, matched 6, offset 0, limit 100, shown 6
/factories/Synthetic project one/
  thr_syn_0001 · status idle · age 1m · location cloud · parent none · title Synthetic session 1
/factories/Synthetic project two/
  thr_syn_0002 · status active · age 2m · location local on Synthetic machine one · parent none · title Synthetic session 2
/factories/_ungrouped/
  thr_syn_0003 · status error · age 3m · location cloud · parent none · title Synthetic session 3 · ungrouped: project prj_syn_0099 is not in the project list read in this run
...
```

The text layout may still change. Scripts should use `--json`.

Running `openwiz ls` or `show` reads your real sessions through your signed-in `room-cli`. The tests never do: they run a fake `room-cli` instead.

### Limits

- **CloudRoom only, read only.** No other service, and no way to act on a session.
- **Checked against real CloudRoom data once, on one Mac,** with a build that differs from this release only in its tested-version label ([evidence](docs/evidence/0.0.1-live-check.md)). *Updated for 0.0.2:* then once more, on the final build ([evidence](docs/evidence/0.0.2-live-check.md#001s-live-check-rerun-on-its-final-build)). The fixture tests pass on Linux; on macOS they last ran on an earlier 0.0.1 build. Windows is untested.
- **Not proven:** that a read can never wake a session or mark it read. The live check found no such effect, but it was limited: [its limits](docs/evidence/0.0.1-live-check.md#limits-plainly).

## The larger design (proposal only)

The 0.0.0 design documents are kept as they were, except where 0.0.1 or 0.0.2 needed to say something new; each such sentence is marked *Updated for 0.0.1* or *Updated for 0.0.2*.

OpenWiz proposes a **coordination layer above the personal agents a person already uses**. A user could start with one personal agent and add more later. The agents would share memory kept in the user's own private repository. Cloud coding factories would sit downstream and do bounded software work when asked.

It is not one personal agent that happens to launch coding agents. The aim is for personal agents to work from the same memory and hand work to each other and to coding factories, without the user copying context between them or managing terminals by hand.

OpenWiz would be **CLI-first and agent-first**, with a **polished UI** as an equal goal. Users would drive it from a command line, through commands and buttons in their personal agent, or from the web.

- **The core, the CLI and all backend logic would be written in Rust.**
- **The controller** is the one Rust core that owns authoritative state, permissions, routing and coordination. Each project's **orchestrator** runs that project's factory workflow under the controller. It is not a second authority.
- **OpenWiz Web** would be a Next.js and TypeScript client of that same backend. It would be a live graph and control surface, where every button is one backend operation.
  - It would show what is running, blocked or done, loop and review progress, and policy decisions, linking to the real underlying artifacts.
  - It would keep only presentation state, such as which screen is open, and never authoritative product state.
- **Desktop packaging** is undecided.
- Everything would live in **one monorepo**: a Rust workspace for the core, the CLI and the service adapters, `apps/web` for the Next.js client, and shared contracts, tests and docs. The layout and dependency rules are in [docs/architecture.md](docs/architecture.md#5-proposed-repository-layout-and-dependency-direction).

None of these interfaces exists yet, apart from 0.0.1's read-only command above. *Updated for 0.0.2:* and 0.0.2's read-only server and web page, which show the inventory and nothing else: no graph, no buttons and no control.

![OpenWiz proposed product architecture. Personal agents, including dot and Grok Bot, connect through the draft PACP protocol to the OpenWiz Controller: one CLI-first Rust core that owns state, permissions, routing and coordination. The controller links to shared memory in the user's private GitHub repo and to OpenWiz Web, a Next.js client that keeps presentation state only. Over an unverified route it sends work to per-project factories on CloudRoom. In each one, a project orchestrator works under the controller. It defines proof (claims and evidence) before build, then runs test and review, loops back through rework to test, and ends at a policy gate.](docs/assets/openwiz-architecture.png)

*Product architecture · the 0.0.0 design, unchanged. Everything shown is proposed. Dashed links are unverified. Editable source: [openwiz-architecture.svg](docs/assets/openwiz-architecture.svg).*

![OpenWiz planned monorepo modules. The thin clients crates/cli and apps/web, which keep presentation state only, call the same operations in crates/core, the controller. The web also uses generated types from contracts/. crates/core holds operations and rules and depends on contracts/. crates/adapters/*, one per external service, implement core interfaces, keep integration bookkeeping only, never own policy, and would talk to external services: CloudRoom, GitHub, personal agents via PACP and the memory repository. No OpenWiz adapter is built yet. tests/ check every surface. Desktop packaging comes later.](docs/assets/openwiz-modules.png)

*Planned monorepo modules · the 0.0.0 design, unchanged. 0.0.1 has built a first part of `crates/core`, one CloudRoom adapter, `crates/cli`, one contract and `tests/`; nothing else. *Updated for 0.0.2:* 0.0.2 adds `crates/server` (a second thin client of the core, like the CLI), a first, read-only `apps/web`, and the web API contract. Dependencies point inward. Editable source: [openwiz-modules.svg](docs/assets/openwiz-modules.svg).*

Detailed diagrams: [docs/architecture.md](docs/architecture.md). Proposed commands and screens: [docs/interface.md](docs/interface.md).

### First pilot (proposed)

The first pilot is **dot → a controller on a user-owned PC → a CloudRoom coding factory**.

- The PC is the controller host chosen for this pilot. Users should not need an always-on machine of their own.
- **A proposed, untested alternative** is an event-driven controller hosted in CloudRoom, using its [automations](https://www.cloudroom.dev/docs/automations) and [subagents](https://www.cloudroom.dev/docs/subagents).
  - Each run would read its state from GitHub and the issue tracker and write it back.
  - This alternative assumes no stable inbound endpoint and no always-on server.
  - Starting work while the user's own computer is off has not been verified.
- The first personal agents to integrate are **dot** and **Grok Bot**. **Muse** and **Duet** are planned later. These are targets. None is a verified integration.

### What users would bring

- A **GitHub** account. GitHub is required at first.
- Their existing **issue tracker**. GitHub Issues is one option, not the only one intended.
- Their existing subscriptions, personal agents, agent harnesses and compute.

Managed hosting and selling compute are possible paid offerings for later. Neither exists, and neither is a launch requirement.

## What is in this repository

| Path | What it is |
| --- | --- |
| `crates/core`, `crates/adapters/cloudroom`, `crates/cli`, `Cargo.toml`, `Cargo.lock` | The Rust workspace: the inventory rules, the read-only CloudRoom reader and the `openwiz` command |
| `crates/server` | `openwiz-server` (0.0.2): the read-only HTTP API on `127.0.0.1`, and the web page with `--web-root` |
| `apps/web` | The read-only web page (0.0.2): a Next.js static export, with its lockfile, its bundled fonts and their licence ([apps/web/public/fonts/OFL.txt](apps/web/public/fonts/OFL.txt)) |
| [contracts/inventory.v1.schema.json](contracts/inventory.v1.schema.json) | The JSON Schema of `openwiz ls --json` and `show --json`, and of the web API's answers |
| [contracts/web-api.v1.md](contracts/web-api.v1.md) | The web API (0.0.2): routes, parameters, statuses, headers and bounds |
| `tests/` | Proof tests, checks, a fake `room-cli` and synthetic fixtures ([tests/fixtures/README.md](tests/fixtures/README.md)) |
| [docs/dependencies.md](docs/dependencies.md) | Every locked crate with its version, licence and checksum, the web page's packages, and the tools needed |
| [docs/web-access.md](docs/web-access.md) | How to reach the page from your phone through a Cloudflare Tunnel and Access, and how it is protected |
| [docs/evidence/0.0.1-live-check.md](docs/evidence/0.0.1-live-check.md) | What 0.0.1 was tested with and what is not proven, and its live check: method, rules, environment, results as counts, and limits |
| [docs/evidence/0.0.2-live-check.md](docs/evidence/0.0.2-live-check.md) | The same for 0.0.2, and the rerun of 0.0.1's check on its final build |
| [docs/overview.md](docs/overview.md) | The problem, the layers, interfaces, built-in plans, approval policy, terms and principles |
| [docs/interface.md](docs/interface.md) | The proposed command set, decision types and screens shared by the CLI, agent commands and OpenWiz Web |
| [docs/workflow.md](docs/workflow.md) | How the controller and the factory loop work: ownership, ticket states, rework, pause and resume, approval, dependent unlock, a worked example, and proof-driven development |
| [docs/architecture.md](docs/architecture.md) | Mermaid diagrams: today's manual workflow, the first pilot, the broader architecture, the release flow, and the repository layout with its dependency direction |
| [docs/pacp/v0.1.md](docs/pacp/v0.1.md) | **PACP v0.1** (Personal Agent Connection Protocol): a draft protocol proposal with synthetic examples and proposed conformance checks. Nothing implements it |
| [docs/decisions.md](docs/decisions.md) | Decisions made so far, and the ones still open |
| [CHANGELOG.md](CHANGELOG.md) | What each version adds, in a short list |
| [CONTRIBUTING.md](CONTRIBUTING.md) | How to give feedback and contribute |
| [RELEASING.md](RELEASING.md) | Version lines, what a release means here, and how one is reviewed |
| [LICENSE](LICENSE) | Apache License 2.0 |

## Status words

| Label | Meaning |
| --- | --- |
| **Released** | In a published OpenWiz version, with what was tested and what is unproven stated in its evidence page under [docs/evidence/](docs/evidence/0.0.1-live-check.md). *(New in 0.0.1.)* |
| **Current** | What a person does by hand today. No OpenWiz software is involved. |
| **Unverified** | An integration that might be possible but has not been tested or proven. |
| **Proposed** | Design only. Nothing has been built. |
| **Future** | Planned for later, or deliberately out of scope for now. |

| Part | Status |
| --- | --- |
| Manual workflow: assistant → person → local CLI → cloud coding sessions | Current |
| Controller (one Rust core), CLI, agent commands and buttons | Proposed |
| Command set, decision types and screens ([docs/interface.md](docs/interface.md)) | Proposed |
| Controller and factory workflow ([docs/workflow.md](docs/workflow.md)) | Proposed |
| OpenWiz Web (Next.js client: live graph and control surface) | Proposed. *Updated for 0.0.2:* a first, read-only page of the inventory is Released (0.0.2, below); the graph and every control are still proposed |
| Monorepo layout: Rust workspace (core, CLI, adapters), `apps/web`, shared contracts, tests and docs | Proposed. 0.0.1 has the Rust workspace's first crates, one contract and tests; no `apps/web`. *Updated for 0.0.2:* 0.0.2 adds `crates/server`, a first `apps/web` and the web API contract |
| Desktop packaging | Undecided |
| `openwiz ls`, `ls -l`, `show`, `--json`: a read-only Rust inventory of real CloudRoom sessions, through `room-cli` ([changelog](CHANGELOG.md), [decisions](docs/decisions.md#001-a-read-only-inventory)) | Released (0.0.1). Fixture-tested; [one live check on one Mac](docs/evidence/0.0.1-live-check.md), with its limits; [not proven](docs/evidence/0.0.1-live-check.md#not-proven). *Updated for 0.0.2:* [rerun on its final build](docs/evidence/0.0.2-live-check.md#001s-live-check-rerun-on-its-final-build), passed |
| `openwiz-server` and its web page: the same inventory, read-only, on your phone through a Cloudflare Tunnel and Access ([changelog](CHANGELOG.md), [setup](docs/web-access.md)) | Released (0.0.2). Fixture-tested, in Chromium for the page; [one live check on one Mac](docs/evidence/0.0.2-live-check.md), with its limits; [not proven](docs/evidence/0.0.2-live-check.md#not-proven) |
| Memory captured automatically in the user's private repository | Proposed. Detailed behaviour is left to a separate design session. |
| Built-in plans: Grill or Wayfinder → plan → user approval → launch | Proposed |
| Controller on a user-owned PC for the pilot | Proposed |
| Event-driven controller hosted in CloudRoom | Proposed, untested |
| Control route from the controller to CloudRoom | Unverified |
| OpenWiz adapters for CloudRoom, GitHub, personal agents and the memory repository ([access vs adapters](docs/architecture.md#external-services-access-versus-adapters)) | Proposed. Only a read-only CloudRoom reader exists (0.0.1); no control of CloudRoom, and no other adapter |
| dot and Grok Bot integrations | Proposed targets, unverified |
| Muse and Duet integrations | Future (planned later) |
| Managed hosting and selling compute | Future (possible paid offerings, not implemented) |
| PACP v0.1 | Proposed (draft) |
| Business multiplayer (shared organisational memory, many owners) | Future |

## Separate things, separate versions

- **OpenWiz** is the product. This is version **0.0.2**: the read-only inventory above, in the terminal (0.0.1, its first version with software) and as a web page (0.0.2). 0.0.0 was documents only.
- **PACP** is a proposed protocol with its own version line, currently **v0.1 (draft)**. It is meant to stand apart from OpenWiz so that other systems could implement it.
- **Adapters** would connect particular personal agents to PACP. None exists. Each would have its own version.

These version numbers move independently, and none implies compatibility with another. A release that contains software states its real dependencies explicitly, for example which PACP version it implements. See [RELEASING.md](RELEASING.md).

## License

[Apache License 2.0](LICENSE).
