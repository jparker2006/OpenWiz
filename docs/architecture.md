# Architecture diagrams

These diagrams are Mermaid source; GitHub renders them in place. Each node is labelled **Current**, **Unverified**, **Proposed**, **Deferred** or **Future**. Dashed outlines mark anything that is not current behaviour.

![OpenWiz proposed product architecture, summarising diagrams 2 and 3 below.](assets/openwiz-architecture.png)

*Product architecture image. Editable source: [openwiz-architecture.svg](assets/openwiz-architecture.svg). The Mermaid diagrams below are the detailed versions.*

## 1. Current: the manual workflow

**Status: Current.** This is how work moves today, entirely by hand. No OpenWiz software is involved. The person is the integration layer.

```mermaid
sequenceDiagram
  autonumber
  actor Person
  participant PA as Personal assistant
  participant CLI as Local coding CLI
  participant Cloud as Cloud coding sessions
  participant Git as Git host
  Person->>PA: Describe the goal
  PA-->>Person: Draft a plan or prompt
  Person->>CLI: Paste the prompt and run a command
  CLI->>Cloud: Start one or more sessions
  Cloud->>Git: Push a branch or open a pull request
  Cloud-->>Person: Status and summary, read in the cloud tool
  Person->>PA: Paste the results back for the next step
  Note over Person,PA: Each assistant keeps its own memory. Context moves only when the person copies it.
```

## 2. First pilot: dot → controller on a user-owned PC → CloudRoom factory

**Status: Proposed. The control route is unverified. Nothing here has been built or accepted.** The PC is the controller host chosen for this pilot. Users should not need an always-on machine of their own. An event-driven controller hosted in CloudRoom is a proposed, untested alternative (dotted path). dot is an integration target, not a verified integration, and its commands and buttons do not exist yet.

```mermaid
flowchart TB
  USER["User"]
  DOT["dot, personal agent<br/>controller commands and buttons: PROPOSED, not built"]
  subgraph PLAN["Built-in plan, PROPOSED"]
    direction LR
    IDEA["Grill or Wayfinder"] --> PLANDOC["Plan: a version, spec or tickets"]
  end
  PLANOK{"User approves the plan<br/>covers this plan only, not every action"}
  subgraph PC["User-owned PC: chosen for this pilot, not required for everyone"]
    CTRL["Controller: one Rust core, PROPOSED<br/>authoritative state, permissions, routing, coordination<br/>one owner at a time, explicit handoff"]
    LOG[("Durable request and receipt log, PROPOSED")]
    REG[("Session registry, PROPOSED<br/>roles and observed configuration")]
  end
  ROUTE["Control route to CloudRoom<br/>UNVERIFIED"]
  ALT["Alternative host, PROPOSED, UNTESTED<br/>event-driven controller in CloudRoom automations and subagents<br/>state in GitHub and the issue tracker, no inbound endpoint assumed"]
  subgraph FACTORY["CloudRoom factory: bounded loop per ticket attempt, PROPOSED"]
    direction LR
    ORCH["Project orchestrator<br/>runs the workflow under the controller"] --> PROOF["Define proof<br/>claims and required evidence"] --> BUILD["Build"] --> TEST["Test"] --> REVIEW["Independent review"]
    REVIEW -->|"finding open, rounds left"| REWORK["Rework"]
    REWORK --> TEST
  end
  GATE{"Policy gate, set by the user<br/>request approval, or proceed only under<br/>applicable prior authorization"}
  ESC["Stop and escalate to the user"]
  UNSUP["Unsupported or uncertain<br/>reported as such, never emulated"]
  MERGE["Merge on GitHub"]
  CLEAN["Automatic destructive workspace cleanup<br/>DEFERRED, owner-approved, integration pending"]

  USER -->|"decides through commands and buttons"| DOT
  DOT --> IDEA
  PLANDOC --> PLANOK
  PLANOK -->|"changes requested"| IDEA
  PLANOK -->|"approved"| CTRL
  PLANOK -.->|"approved, alternative host"| ALT
  ALT -.->|"start sessions"| ORCH
  CTRL --> LOG
  CTRL --> REG
  CTRL -->|"start, reuse, drain or rotate sessions"| ROUTE
  ROUTE --> ORCH
  ROUTE -.->|"route unavailable or outcome unknown"| UNSUP
  REVIEW -->|"passes, reaches a checkpoint"| GATE
  REVIEW -->|"finding still open after 2 + 1 rounds"| ESC
  GATE -->|"approval requested"| DOT
  GATE -->|"approved, or prior authorization applies"| MERGE
  GATE -->|"changes requested"| REWORK
  ESC --> DOT
  UNSUP --> DOT
  CTRL -.-> CLEAN

  classDef proposed stroke-dasharray: 5 5
  class DOT,IDEA,PLANDOC,PLANOK,CTRL,LOG,REG,ROUTE,ALT,ORCH,PROOF,BUILD,TEST,REVIEW,REWORK,GATE,ESC,UNSUP,MERGE,CLEAN proposed
```

Notes:

- **Gates are policy, not fixed steps.** The user decides which checkpoints, merges included, request approval and which may proceed under an applicable prior authorization. Approving the plan does not authorise every action the plan leads to.
- **User control has limits.** No policy can exceed provider permissions, platform restrictions or applicable safety requirements.
- Requests and receipts are logged durably, so an interrupted or duplicated request is reconciled rather than repeated blindly.
- The builder and the independent reviewer are separate sessions with recorded roles.
- Ticket states, rework, pause and resume, and dependent unlock are described in [workflow.md](workflow.md).
- Automatic destructive cleanup of workspaces is deferred. Until it exists, cleanup is a manual action.
- The alternative host would rely on CloudRoom's documented [automations](https://www.cloudroom.dev/docs/automations) and [subagents](https://www.cloudroom.dev/docs/subagents). It is untested for this use. It promises no stable inbound endpoint, no always-on sandbox server, and no verified launch while the user's own computer is off.

## 3. Proposed: personal agents, shared memory and coding factories

**Status: Proposed. Integrations are unverified targets. Business multiplayer is Future.** A user can start with one personal agent. No personal-agent interface is assumed to exist: each adapter needs a supported, documented interface that the user is allowed to use.

```mermaid
flowchart TB
  USER["User<br/>sets policy, grants and revokes access, decides when asked"]
  CLI["OpenWiz CLI and agent commands<br/>primary interfaces, Rust core, PROPOSED"]
  WEB["OpenWiz Web, Next.js client, PROPOSED<br/>live graph and control surface, same backend and operations<br/>not a separate state authority"]
  TRACK[("User's existing issue tracker<br/>GitHub Issues or another")]
  LIMITS["Provider permissions, platform restrictions,<br/>applicable safety requirements"]

  subgraph AGENTS["Personal agents: start with one, add more later"]
    PA1["dot<br/>initial target, UNVERIFIED"]
    PA2["Grok Bot<br/>initial target, UNVERIFIED"]
    PA3["Muse, Duet<br/>planned later, FUTURE"]
  end

  subgraph ADAPT["Adapters, PROPOSED: none verified"]
    AD1["Adapter for dot"]
    AD2["Adapter for Grok Bot"]
    AD3["Later adapters"]
  end

  subgraph FLEET["Controller: one Rust core, PROPOSED<br/>authoritative state, permissions, routing, coordination"]
    POL["Permissions: policy and grants<br/>gates, prior authorizations, revocation"]
    COORD["Routing and coordination<br/>plans, delegation, handoff, status, state"]
    BROKER["Memory broker<br/>scoped references, automatic capture"]
    AUDIT[("Provenance and audit log")]
  end

  MEM[("User's private repository<br/>memory captured automatically, ongoing domain onboarding<br/>detailed behaviour: separate design session")]

  subgraph DOWN["Downstream coding factory on the user's own subscriptions and compute, PROPOSED"]
    ORCH["Project orchestrator, one per factory<br/>runs the factory workflow under the controller<br/>holds no authoritative state"]
    subgraph RUNLOOP["Bounded loop"]
      direction LR
      P0["Define proof"] --> B["Build"] --> T["Test"] --> R["Review"]
      R -->|"finding open, rounds left"| W["Rework"]
      W --> T
    end
    GATE{"Policy gate<br/>request approval, or proceed only under<br/>applicable prior authorization"}
    STOP["Stop and escalate to the user"]
  end

  GIT["GitHub, required at first"]
  FUT["Business multiplayer<br/>FUTURE SCOPE"]

  PA1 --- AD1
  PA2 --- AD2
  PA3 -.- AD3
  AD1 -->|"PACP v0.1 draft"| COORD
  AD2 -->|"PACP v0.1 draft"| COORD
  AD3 -.->|"PACP v0.1 draft"| COORD
  USER --> CLI
  USER --> WEB
  CLI -->|"operations"| COORD
  WEB -->|"same operations"| COORD
  COORD -->|"running, blocked, done, loop and review progress, policy decisions"| WEB
  COORD ---|"tickets"| TRACK
  USER --> POL
  LIMITS -.->|"bound every policy"| POL
  COORD --> POL
  COORD --> BROKER
  BROKER ---|"scoped reads, captured writes"| MEM
  COORD --> AUDIT
  BROKER --> AUDIT
  COORD -->|"approved plan, bounded"| ORCH
  ORCH --> P0
  R -->|"passes, reaches a checkpoint"| GATE
  R -->|"finding still open after 2 + 1 rounds"| STOP
  POL -.->|"gate rules"| GATE
  GATE -->|"approval requested"| COORD
  GATE -->|"approved, or prior authorization applies"| GIT
  STOP --> COORD
  ORCH -->|"status, results, unsupported or uncertain"| COORD
  FUT -.- FLEET

  classDef proposed stroke-dasharray: 5 5
  class CLI,WEB,PA1,PA2,PA3,AD1,AD2,AD3,POL,COORD,BROKER,AUDIT,MEM,ORCH,P0,B,T,R,W,GATE,STOP,FUT proposed
```

OpenWiz Web reads and acts through the same operations as the CLI and agent commands. When implemented, each item it shows would link to the real underlying artifact.

Memory and context windows are deliberately separate. An agent receives scoped references, and only the content its grant allows. What lands in its context window is a transient copy. How capture, consent, conflicts, encryption and retention work is left to a separate memory design session.

## 4. Proposed: private development to public release

**Status: Proposed process.** The private development repository and the public repository share no Git history. Public files are written fresh and pass an allowlisted export review.

```mermaid
flowchart TD
  SRC["Other private sources<br/>read-only reference, nothing copied"]
  DEV["Private development and private beta<br/>notes, drafts, experiments, beta builds"]
  CAND["Candidate public change<br/>written fresh, synthetic examples only"]
  ALLOW{"Allowlist check<br/>only approved file paths"}
  SCAN{"Privacy, secret and private-link scan<br/>plus claim and status check"}
  MAINT{"Maintainer approves exact file list and diff"}
  PUB["Public repository<br/>ordinary commit, no force-push"]
  VERIFY["Read back the remote tree and compare"]
  TAG["Tag and release notes<br/>only for an approved release"]

  SRC -.->|"reference only"| DEV
  DEV --> CAND
  CAND --> ALLOW
  ALLOW -->|"file not on the allowlist"| CAND
  ALLOW --> SCAN
  SCAN -->|"finding"| CAND
  SCAN --> MAINT
  MAINT -->|"changes requested"| CAND
  MAINT -->|"approved"| PUB
  PUB --> VERIFY
  VERIFY -->|"mismatch"| MAINT
  VERIFY --> TAG
```

## 5. Proposed: repository layout and dependency direction

**Status: Proposed. Nothing exists yet:** no workspace, crate, app or test. The names below are illustrative.

*Updated for 0.0.1:* `Cargo.toml`, `crates/core`, `crates/adapters/cloudroom`, `crates/cli`, `contracts/inventory.v1.schema.json` and `tests/` now exist, for the read-only inventory only. `apps/web`, every other adapter, and the rest of the core do not. *Updated for 0.0.2:* `crates/server`, a first, read-only `apps/web` and `contracts/web-api.v1.md` exist too. The server depends on the core and the CloudRoom adapter, as the CLI does, and nothing depends on the server; the page talks to the server only over HTTP.

OpenWiz is planned as **one monorepo**. The private beta uses this same layout. It is the same product, not a second implementation.

![OpenWiz planned monorepo modules and their dependency direction, as described in this section.](assets/openwiz-modules.png)

*Editable source: [openwiz-modules.svg](assets/openwiz-modules.svg).*

```text
OpenWiz/
├── Cargo.toml          Rust workspace
├── crates/
│   ├── core/           behaviour: operations, state, revisions, policy gates, honest lists
│   ├── adapters/       one crate per external service: CloudRoom, GitHub, personal agents over PACP, the memory repository
│   └── cli/            thin client: parses commands, prints results
├── apps/
│   └── web/            thin client: Next.js and TypeScript
├── contracts/          shared, language-neutral operation and data schemas, decision types, PACP schemas, test fixtures
├── tests/              cross-component and contract-conformance tests
└── docs/               product and protocol documents
```

Desktop packaging comes later, and its form is undecided.

### Who owns what

| Module | Owns | Never does |
| --- | --- | --- |
| `crates/core` | Every operation and its rules: listing, inspection, changes with revision checks, decisions, policy gates, reconciliation of uncertain outcomes. It defines the interfaces that adapters implement | Talk to an external service directly, or format output for a person |
| `crates/adapters/*` | Talking to one external service each, translating its data, and reporting "unsupported" or "uncertain" honestly. They may keep **integration bookkeeping**, such as cursors, request IDs and retry records | Own or decide policy, become the source of truth for product state, or call another adapter |
| `crates/cli` | Turning typed commands into core operations and printing the results. It may keep **presentation state**, such as the current path | Contain product rules or authoritative product state |
| `apps/web` | Rendering screens and turning each button into one operation. It may keep **presentation state**, such as the open screen or an unsent note | Hold authoritative product state, contain product rules, have web-only operations, or call services or adapters directly |
| `contracts/` | The shapes every part agrees on, and shared fixtures | Contain behaviour |
| `tests/` | Checking that every surface gives the same result for the same operation | Ship in a release |

### Dependency direction

Dependencies point inward, toward the core and the contracts. No module depends on a client.

```mermaid
flowchart TB
  CLI["crates/cli<br/>thin client"]
  WEB["apps/web, Next.js<br/>thin client"]
  CORE["crates/core<br/>operations and rules"]
  AD["crates/adapters/*<br/>one per external service"]
  CON["contracts/<br/>shared schemas and fixtures"]
  EXT["External services<br/>CloudRoom, GitHub, personal agents, memory repository"]
  TEST["tests/"]

  CLI -->|"calls operations"| CORE
  WEB -->|"calls the same operations, served by the Rust backend"| CORE
  WEB -->|"generated types"| CON
  AD -->|"implements interfaces defined by"| CORE
  CORE --> CON
  AD --> CON
  AD -->|"talks to"| EXT
  TEST -.->|"checks"| CLI
  TEST -.->|"checks"| WEB
  TEST -.->|"checks"| CORE

  classDef proposed stroke-dasharray: 5 5
  class CLI,WEB,CORE,AD,CON,TEST proposed
```

- **One set of operations.** The CLI, agent commands and the web all reach the same core operations, so a change behaves the same wherever it starts.
- **Wiring happens in a binary.** The binary that runs the controller connects the chosen adapters to the core.
- **The controller is `crates/core` plus the binary that runs it.** It is the one Rust core that owns authoritative state, permissions, routing and coordination. Project orchestrators run each factory's workflow under it, through its operations.
- **Still open:** how the web reaches the backend (the transport and its served interface) and the name of the CLI program. *Updated for 0.0.1:* 0.0.1's program is `openwiz`. *Updated for 0.0.2:* 0.0.2's read-only page reaches `openwiz-server` with JSON over HTTP ([contract](../contracts/web-api.v1.md)); the controller's own interface is still open.

### External services: access versus adapters

Access that a service offers today is not an OpenWiz integration. **No OpenWiz adapter exists.** *Updated for 0.0.1:* one exists now, the read-only CloudRoom reader in `crates/adapters/cloudroom`; no other. Each one must be built and verified under [proof-driven development](workflow.md#proof-driven-development) before anything is claimed.

| Service | Access available today, outside OpenWiz | OpenWiz adapter |
| --- | --- | --- |
| CloudRoom | CloudRoom's own app and CLI. Its documented [automations](https://www.cloudroom.dev/docs/automations) and [subagents](https://www.cloudroom.dev/docs/subagents) | Proposed, not implemented. The control route from a controller is unverified. *Updated for 0.0.1:* reading sessions through `room-cli` is released in 0.0.1 (read only); controlling CloudRoom is not implemented |
| GitHub | Git, and GitHub's documented [REST API](https://docs.github.com/en/rest) and [GraphQL API](https://docs.github.com/en/graphql) | Proposed, not implemented |
| Memory repository | A private repository on GitHub, through the same access | Proposed, not implemented. Its behaviour is left to the memory design session |
| Personal agents (dot, Grok Bot; Muse and Duet later) | Only through an interface the vendor supports and the user may use. None is verified for this purpose | Proposed, not implemented. Each requires a supported interface |
