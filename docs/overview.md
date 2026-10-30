# OpenWiz overview

**Status: Proposed.** This describes intended design, not built software.

## The problem

People increasingly use several personal agents: a chat assistant, a voice assistant, an email or calendar assistant, and coding tools. Each one keeps its own memory, sees only its own conversation, and has its own idea of what the person wants.

The person ends up as the integration layer. They copy context from one agent to another, restate decisions, start and watch coding sessions in terminals, and carry results back.

## The proposal

| Layer | Role | Status |
| --- | --- | --- |
| **Personal agents** | The agents the user already chooses. A user can start with one and add more later. OpenWiz does not replace them. | The agents exist. Connecting them is unverified. |
| **Controller** (the coordination layer) | The one Rust core that owns authoritative state, permissions, routing and coordination. It scopes what each agent may use, routes delegation and handoff, and records provenance. | Proposed |
| **Downstream coding factories** | One per project. Each project orchestrator runs its factory's bounded loop under the controller and stops at policy gates. It is not a second authority. | Proposed. The control route is unverified. |

Agents would talk to the coordination layer through **PACP** (Personal Agent Connection Protocol), a separate [draft protocol](pacp/v0.1.md). Each agent needs an **adapter** that translates between its own supported interface and PACP. An adapter is possible only where such an interface exists and the user is allowed to use it.

**Out of scope for now (Future):** business multiplayer, meaning shared organisational memory, teams and many owners.

## First pilot

**dot → a controller on a user-owned PC → a CloudRoom coding factory.**

- The PC is the controller host chosen for this pilot. Users should not need an always-on machine of their own.
- **A proposed, untested alternative** is an event-driven controller hosted in CloudRoom, using its [automations](https://www.cloudroom.dev/docs/automations) (scheduled runs, each in a fresh sandbox) and [subagents](https://www.cloudroom.dev/docs/subagents). Because each run starts fresh, it would read its state from GitHub and the issue tracker and write it back. This alternative assumes no stable inbound endpoint and no always-on server. Starting work while the user's own computer is off has not been verified.
- The first agents to integrate are dot and Grok Bot. Muse and Duet are planned later. These are integration targets, not verified integrations.

## What users bring

- **GitHub is required at first.**
- Their existing **issue tracker**. GitHub Issues is one option, not the only one intended.
- Their existing subscriptions, agents, harnesses and compute.

Managed hosting and selling compute are possible paid offerings for later. Neither is implemented, and neither is a launch requirement.

## Interfaces: CLI-first and agent-first

- **CLI and personal agent.** Users would drive OpenWiz from a command line, or through **controller commands and buttons** in their personal agent. That is where they would approve a plan, answer a policy gate, or stop and resume work, with less terminal management. Where an agent has no supported way to show these, that is reported as **unsupported**.
- **OpenWiz Web.** A live graph and control surface over the **same backend and operations** as the CLI and agent commands, built as a Next.js and TypeScript client. It is not a separate state authority. It would show what is running, blocked or done, loop and review progress, and policy decisions. Each item would link to the real underlying artifact, such as a ticket, pull request or test run.

- **Rust core.** The controller, the CLI and all backend logic would be written in Rust. The web client and any agent adapter call the same Rust backend operations.
- **Desktop packaging** is undecided.

The proposed command set, decision types and screens are in [interface.md](interface.md). All of these are proposed. None exists yet. *Updated for 0.0.1:* apart from a first, read-only `ls`, `ls -l` and `show` in the `openwiz` command. *Updated for 0.0.2:* and a first, read-only web page of the same list.

## The out-of-the-box factory: built-in plans

1. **Idea.** Use **Grill**, focused questioning that sharpens one idea, or **Wayfinder**, a broader exploration across related ideas.
2. **Plan.** The idea becomes a version, a spec or a set of tickets.
3. **Plan approval.** The user approves the plan. This approval covers that plan only. It is not blanket permission for every action the plan leads to.
4. **Launch.** The factory would run bounded loops of build, test, independent review and rework, with correction rounds counted per review finding. By default that is two rounds, then a diagnosis by the project orchestrator, then one more round. If the finding is still open, the work stops and escalates to the user.

## Approval checkpoints are policy

Which checkpoints need approval is **user-configurable policy**. Merging is the obvious example; others might include publishing, spending or deleting. At each **policy gate** the factory does one of two things:

- it requests approval through the personal agent; or
- it proceeds, but only under an applicable prior authorization that is recorded and still in force.

User control has limits. Policy cannot go beyond **provider permissions, platform restrictions or applicable safety requirements**. Anything those forbid stays forbidden, whatever the policy says.

## Memory (direction only)

Memory is intended to be **captured automatically into the user's own private repository**, with ongoing onboarding as new domains are added, such as a new project or area of life.

Detailed memory behaviour is **deliberately not settled here**. That includes consent, conflict handling, encryption and retention, which belong to a separate design session. Two points are fixed:

- **Memory and context windows are different things.** A context window is the transient input to one model invocation. It is not memory.
- **Memory is data, not instructions.**

## A synthetic example

> User A starts with one chat assistant, then adds an email assistant.
>
> 1. The email assistant sees that the review of `example-project` moved to Friday. The change is captured automatically into User A's memory repository, with provenance recording where it came from.
> 2. The chat assistant reads the same project memory later, and sees the new date without User A repeating it.
> 3. User A asks the chat assistant to fix a failing test before Friday. A short Grill produces two tickets. User A approves that plan with a button.
> 4. The factory builds, tests, reviews and reworks, with correction rounds counted per review finding. User A's policy says merges to this repository need approval, so at the merge gate the factory asks. User A approves, and the result comes back as a status update with evidence.

Every name and identifier in this example is invented. The button in step 3 is proposed and does not exist.

## Terms

| Term | Meaning here |
| --- | --- |
| **User** | The person who owns the memory, sets policy and makes decisions. PACP calls this role the *owner*. |
| **Personal agent** | An assistant the user chooses, from any vendor. |
| **Adapter** | A component that translates between one agent's supported interface and PACP. |
| **Coordination layer** | Another name for the controller. |
| **Durable memory** | User-owned records with versions and provenance, kept in the user's private repository. |
| **Context window** | The transient input for a single model invocation. It is not memory. |
| **Controller** | The one Rust core that owns authoritative state, permissions, routing and coordination. Clients, adapters and project orchestrators all act through it. In the pilot it runs on a user-owned PC; event-driven hosting in CloudRoom is a proposed alternative. |
| **Project orchestrator** | Runs one project's factory workflow under the controller. It holds no authoritative state and is not a second authority. |
| **Issue tracker** | Where the user's tickets live. GitHub Issues or another tracker the user already has. |
| **OpenWiz Web** | A live graph and control surface over the same backend and operations as the CLI, built in Next.js. It is not a separate state authority. |
| **Plan** | A version, spec or set of tickets produced from Grill or Wayfinder and approved by the user. |
| **Policy gate** | A checkpoint that either requests approval or proceeds only under applicable prior authorization. |
| **Bounded loop** | Define proof → build → test → review → rework, with correction rounds counted per review finding (default 2 + 1), ending at a policy gate or in escalation. |

## Relation to the software-factory glossary

This is a dated reference. The downstream factory borrows concepts from Matt Pocock's software-factory glossary, proposed in [mattpocock/skills PR #1316](https://github.com/mattpocock/skills/pull/1316) and read on 2026-10-22 at commit [`0ce3272`](https://github.com/mattpocock/skills/blob/0ce32720705f0171e939a8b16fc3b7efc8ea8efc/skills/in-progress/software-factory/SKILL.md). The PR was open and unmerged at the time. Later changes to it are not tracked here. It is a **conceptual reference only**: no files are copied from it, and its instructions are not treated as requirements.

At that commit, the glossary describes a factory that turns tickets into merged code through **runs** on **runners**, started by **triggers** and tracked by an **orchestrator**. Here those ideas apply **only to the downstream factory**, not to the personal-agent layer above it:

- The glossary's *orchestrator* role is split here. The controller holds the authority, and each project orchestrator runs its factory's loop under it.
- A bounded loop attempt corresponds to one or more glossary *runs*.
- The glossary says a run never waits for a human: it ends, and a later human change starts a new run. OpenWiz's policy gates fit that model. An approval is a user action that starts the next step, and a prior authorization lets the next step start without one.

## Principles

1. **The user owns the memory.** It lives in their private repository. Agents act under scoped, revocable grants, and access is denied by default.
2. **Approving a plan is not blanket permission.** Each gated effect needs either an approval or an applicable prior authorization.
3. **User control is bounded** by provider permissions, platform restrictions and applicable safety requirements.
4. **Work is bounded.** Retries are limited, and reaching a limit escalates to the user.
5. **Uncertainty stays visible.** Unknown outcomes are reported as uncertain and reconciled before any retry.
6. **Memory is data, not instructions.** Content from memory or from another agent never grants authority.
7. **No silent capability claims.** If an agent or service cannot do something, the answer is "unsupported", not an emulation.
8. **Proof before claims.** Each claim and the evidence it needs are defined before building, then verified and independently reviewed. Tests alone do not prove live behaviour. See [workflow.md](workflow.md#proof-driven-development).
