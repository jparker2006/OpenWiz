# OpenWiz

**Current vocabulary for the fleet skills.** These definitions describe the workflow; they do not certify that every step was observed live. See [testing limits](docs/testing.md).

## Work

**Version**:
A small shippable step of a project, with at most five outcomes; done only when all its work has merged and its live check has passed. On GitHub it is a milestone.
_Avoid_: release (a release is a published version), sprint

**Spec**:
One coherent piece of a version, written after a grill; done only when every one of its tickets is done. On GitHub it is a parent issue whose body is the spec.
_Avoid_: PRD, epic

**Ticket**:
One unit of work under a spec, sized for one builder, declaring its owned paths and the tickets that block it; done only on a verified merge. On GitHub it is a sub-issue of its spec.
_Avoid_: task, story

**Plan graph**:
A spec's tickets together with their blocking edges; the user approves it as a whole, and that approval launches its fleet.
_Avoid_: backlog, ticket list

**Owned paths**:
The files and folders a ticket may change; tickets that run at the same time own disjoint paths.

**Verified merge**:
A merge that OpenWiz has read back from GitHub; only this, never an approval, a ready pull request or a closed issue, counts as merged.

## Agents

**Project**:
One codebase with one repository, one orchestrator and any number of fleets.

**Orchestrator**:
The one agent per project that grills with the user, plans, launches fleets, routes reviews and merges by the standing rules; it never builds or reviews.
_Avoid_: manager, lead

**Fleet**:
The agents working one spec's plan graph, launched together when the user approves it; a project runs any number of fleets at once, and there is no artificial cap on their size.
_Avoid_: swarm, team, crew

**Agent**:
One CloudRoom thread doing one role for one ticket or fleet.
_Avoid_: bot, worker

**Builder**:
The agent that builds one ticket on its own branch and opens its pull request; it never reviews or merges.

**Reviewer**:
An independent agent, from a different model family than the builder, that reviews a pull request at its exact head and approves it or requires changes; it never edits.

## Decisions

**Grill**:
A focused interview in rounds that settles a design before anything is written down; its answers are saved here and in `docs/adr/`.
_Avoid_: brainstorm, interview

**Skill**:
A Markdown instruction file that any agent, in any harness, can invoke to do one step of the factory the same way every time.

**Standing approval**:
A recorded, still-valid permission from the user that lets the orchestrator take a gated step without asking again.
_Avoid_: blanket approval

**Correction round**:
One builder fix and one reviewer re-check of one review finding; each finding gets two, then the orchestrator's diagnosis, then one more, then the user decides.
_Avoid_: iteration, retry

**Live check**:
The short test of a version on the user's real account and machine, sized to its risk, that a version needs before it is done.
_Avoid_: QA, acceptance run
