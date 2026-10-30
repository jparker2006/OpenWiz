# Decisions

## Decided for this preview (2026-10-22; revised for OpenWiz)

These are design decisions. None of them is a built feature or a verified integration.

| Decision | Note |
| --- | --- |
| License: Apache License 2.0 | See [LICENSE](../LICENSE) |
| The product is named **OpenWiz** | Renamed from OpenFleet. The protocol keeps the name PACP |
| OpenWiz is CLI-first and agent-first, and a polished UI is an equal goal | Commands and buttons in the personal agent, plus the web client; none exists yet. *Updated for 0.0.1:* 0.0.1 has a first, read-only CLI command, `openwiz` |
| **The controller is the one Rust core that owns authoritative state, permissions, routing and coordination** | Each project orchestrator runs its factory workflow under the controller. It holds no authoritative state and is not a second authority |
| Clients keep presentation state only; adapters keep integration bookkeeping only | Authoritative product state and every policy decision live in the core |
| **Proof-driven development** | Each claim and its required evidence are defined before building, then verified and independently reviewed. Tests alone do not prove live behaviour; nothing is called formally proven without an actual formal proof. See [workflow.md](workflow.md#proof-driven-development) |
| **Correction rounds are counted per review finding (2 + 1)** | Two failed builder and reviewer rounds on one finding, then a diagnosis by the project orchestrator with a helper (not a round), then one more round, then the user's decision. Each finding has its own counter. Both thresholds are configurable whole numbers of at least 1, with defaults 2 and 1. Test runs are not correction rounds. See [workflow.md](workflow.md#failure-and-rework) |
| **At most five outcomes per version**, preferably one to three | No large feature bundle under a vague bullet. See [RELEASING.md](../RELEASING.md#what-a-version-contains) |
| The user merges on GitHub; OpenWiz never performs the merge | One current owner per ticket attempt; dependent tickets unlock once, only after a verified merge. See [workflow.md](workflow.md) |
| **The core, the CLI and all backend logic are written in Rust** | Required. No other language implements backend logic |
| **OpenWiz Web is a Next.js and TypeScript client** | It calls the same Rust backend operations; no web-only operations |
| **One monorepo** | A Rust workspace (`crates/core`, `crates/adapters/*`, `crates/cli`), `apps/web`, `contracts/`, `tests/` and `docs/`. Dependencies point inward: the core holds behaviour, adapters implement its interfaces for external services, and the CLI and web are thin clients of the same operations. The private beta uses the same layout. See [architecture.md](architecture.md#5-proposed-repository-layout-and-dependency-direction) |
| OpenWiz Web is a live graph and control surface over the same backend and operations | It is not a separate state authority. Phone-first, with the same features on desktop |
| One command set, decision types and screens for every surface | See [interface.md](interface.md): list, navigate, inspect and change commands; every change names the revision it saw; five decision types |
| Lists are honest about their data | Completeness, freshness and gaps on every list; unknown sessions stay visible; archived is never shown as success |
| Agents and Memory screens are read-only in the first version | Changes go through the personal agent and the controller's write path |
| Visual style v1 | Dark first; status never relies on colour alone; Geist and Geist Mono; 4-point spacing |
| Version numbers are separate from dependencies | OpenWiz, PACP and adapters have independent versions; real dependencies are listed per release. See [RELEASING.md](../RELEASING.md) |
| A user can start with one personal agent and add more later | |
| First pilot: dot → a controller on a user-owned PC → a CloudRoom coding factory | The PC is chosen for this pilot. Users should not need an always-on machine of their own. |
| Event-driven controller hosting in CloudRoom is a proposed alternative | Untested. No stable inbound endpoint, always-on server or verified launch with the user's computer off is assumed. |
| The first personal agents to integrate are dot and Grok Bot. Muse and Duet are planned later. | Targets, not verified integrations |
| GitHub is required at first | GitHub Issues is not the only intended issue tracker |
| Users bring their existing issue trackers, subscriptions, agents, harnesses and compute | |
| Memory is to be captured automatically into the user's private repository, with ongoing domain onboarding | The detailed behaviour is open (below) |
| The built-in plans run: Grill or Wayfinder → a plan (a version, spec or tickets) → the user approves the plan → the factory launches | Plan approval covers that plan only, not every action |
| Merges and other checkpoints are user-configurable policy | Each gate either requests approval or proceeds only under an applicable prior authorization |
| User control is bounded by provider permissions, platform restrictions and applicable safety requirements | |
| 0.0.0 stays proposal-only, with no installable implementation | |

## 0.0.1: a read-only inventory

*Updated for 0.0.1:* **Status: Released** in 0.0.1. What it does is in the [README](../README.md#what-001-does); what it adds is in the [changelog](../CHANGELOG.md); what was tested, and what is not proven, is in [evidence/0.0.1-live-check.md](evidence/0.0.1-live-check.md). It differs from the plan below in three ways: sessions are grouped by project only (not by version, spec or ticket), unplaced sessions appear under `/factories/_ungrouped/`, and whether a read can wake a session or mark it read is not yet proven.

The plan, as written before it was built: the first runnable version has one job: a **read-only inventory, written in Rust, of the user's real CloudRoom sessions**.

- Each session shows its observed status word, its model where reported, and how fresh the data is.
- A session is grouped under a project, version, spec or ticket **only when that mapping is verified**. Anything else stays visible as unplaced, with its reason. Nothing is guessed.
- Unknowns stay unknown. A missing status, model, parent or freshness is shown as unknown.
- It never starts, wakes, messages, launches or merges anything.

Its release notes will state the CloudRoom CLI version it was tested against and what was tested, as [RELEASING.md](../RELEASING.md) requires.

## Open

| # | Decision | Status | What depends on it |
| --- | --- | --- | --- |
| 1 | **Managed hosting and selling compute** | Possible paid offerings. Not implemented, and not launch requirements. | Business model; support burden |
| 2 | **Memory behaviour**: consent, conflict handling, encryption and retention | Left to a separate memory design session | Memory parts of PACP; the privacy model |
| 3 | **Controller hosting** beyond the pilot | Open. Event-driven CloudRoom hosting is a proposed, untested alternative. | Availability; trust boundary; cost |
| 4 | **Which issue trackers** besides GitHub Issues | Open | Tracker adapters |
| 5 | **How each personal agent would show** the commands and buttons | Open. Depends on each agent's supported interface. The command set itself is proposed in [interface.md](interface.md) | Integration targets |
| 6 | **PACP transport binding**: HTTP plus JSON, a profile of an existing protocol, or several | Open | Implementations; conformance tests |
| 7 | **Identity and authentication** for agents and adapters | Open | Grants; revocation; how far an adapter-only identity can be trusted |
| 8 | **When Muse and Duet are added** | Planned later | Adapters |
| 9 | **Where PACP lives**: in this repository or its own | Open | Governance; independent versioning |
| 10 | **Names**: whether "OpenWiz" and "PACP" are free to use | Not checked | Publishing beyond a preview |
| 12 | **Desktop packaging** | Undecided; planned later, inside the same monorepo | How OpenWiz is installed on a desktop |
| 14 | **How the web reaches the backend** (transport and served interface) | Open | `apps/web`; contract schemas |
| 15 | **Merges without a click**: whether a prior authorization may ever let a merge happen without the user, and who performs it | Unresolved | Policy gates; [workflow.md](workflow.md#human-approval) |
| 16 | **Project orchestrator, implementation details**: where it runs, how it receives work from the controller, and how it reports back. The ownership boundary itself is decided | Unresolved | [workflow.md](workflow.md#who-owns-what) |
| 17 | **Pre-approved dependents**: whether one launch approval may cover a dependent ticket in advance | Unresolved | [workflow.md](workflow.md#dependent-unlock) |
| 13 | **The CLI's program name**, and friendlier names for `launch` and `resume` | Open. *Updated for 0.0.1:* 0.0.1 uses `openwiz` | Command examples |
| 11 | **Business multiplayer** | Future scope | Not before the single-user design is proven |
