# OpenWiz interface: commands, decisions and screens

**Status: Proposed.** This is decided design for the first runnable versions. **Nothing here is built**: no command, button or screen exists. Examples use invented names.

*Updated for 0.0.1:* 0.0.1 has a first, read-only form of `ls`, `ls -l` and `show`, as the program `openwiz` (see the [README](../README.md#what-001-does)). It is narrower than this design: no paths, `cd`, revisions, `next` or `blockers`; sessions are grouped by project only, unplaced ones under `/factories/_ungrouped/`; `ls -l` shows the reported model and effort and the purpose, with no loop step or `Next:`; and `show` gives the final output, not recent messages or evidence. Its text has no status glyphs yet. Nothing else on this page exists. *Updated for 0.0.2:* 0.0.2 adds a first, read-only web page of the same list: groups by project, each status as a word and a glyph, and a details sheet; no buttons, decision sheets or other screens.

## One interface, three surfaces

The CLI, the commands and buttons inside a personal agent, and OpenWiz Web all use **one set of operations** on one controller.

- **The controller is the one Rust core that owns authoritative state, permissions, routing and coordination.**
- **OpenWiz Web** (Next.js) is a thin client:
  - every button or decision sheet maps to exactly one operation below;
  - there are no web-only operations;
  - it keeps presentation state only, such as the open screen, scroll position or an unsent note, and never authoritative product state;
  - it reads nothing directly.
- **People and agents use them differently.** In the GUI people use buttons and decision sheets, and the command text is secondary detail. Agents and scripts use the commands.
- **Merging stays on the code host** (GitHub). OpenWiz never merges.

## One path tree

```text
/factories/<project>/<version>/<spec>/<ticket>/<thread>
/factories/<project>/_unplaced/<session>
/agents/<agent>
/memory/<domain>
```

Every command takes an optional path. `cd` only sets the default path in an interactive shell or a personal agent. Scripts pass full paths.

## Four kinds of command

| Kind | Commands | Rule |
| --- | --- | --- |
| **List** | `ls [path]`, `ls -l [path]`, `next [path]`, `blockers [path]` | Lists what exists and is running. It never wakes, launches, resumes or messages a session. It is cheap enough to poll in a loop. Archived items appear only with `-a`. |
| **Navigate** | `cd <path>`, `cd ..` | Changes focus only. |
| **Inspect** | `show <path>`, `show --all <path>`, `diff <path>`, `plan <path>` | Reads what is stored. Opening something acts on nothing. `plan` shows the plan and its graph: versions, specs and tickets, including ones not yet launched. |
| **Change** | `grill [idea \| issue link]`, `wayfind [idea]`, `launch <path>@rN`, `park <path>@rN`, `resume <path>@rN`, or answering a decision | Explicit, policy-gated and reconciled before any retry. |

**Every change names the revision you saw** (`@rN`). If the item has changed since then, the controller refuses with "changed since you looked: re-review". It never applies the change to the latest version instead.

"Review" means only the factory's independent review step. There is no stop, kill or approve verb. Approving a plan is answering a decision on `plan`. Deferring a decision is `park`.

## Four detail levels

| Level | Command | Adds |
| --- | --- | --- |
| 1 | `ls` | Name, status, revision |
| 2 | `ls -l` | A one-line recap, model and effort, loop step, and `Next:` |
| 3 | `show` | Recent messages, evidence and pending choices |
| 4 | `show --all` | The full stored conversation, as a download |

None of these wakes or messages a session. A missing recap reads "no recap yet". Conversation text never appears in `ls`.

## Decision types

Every decision is exactly one of five types. Each has a subject path, a revision, its options and a rule for notes.

| Type | On screen | Used for |
| --- | --- | --- |
| **Yes / No** | Two large buttons; a note is optional on No | Approvals: a plan, a launch, publish, spend or delete gates |
| **Continue** | One large button | Let paused or held work carry on (`resume`). It continues approved work only and never widens scope |
| **Choose** | Numbered options | Real alternatives, such as a parked review finding or an unplaced session |
| **Answer** | A text box | A Grill or Wayfinder question |
| **Open** | A link | Act elsewhere, for example merge on GitHub. Not a controller decision |

A note attached to a decision is stored with it. It is data and never grants extra authority.

How these decisions move a ticket through its states is in [workflow.md](workflow.md).

## Honest lists

- **Every list reports how good its data is.** That means completeness (complete or partial), freshness (fresh, or stale since a time) and known gaps. "Nothing needs you" appears only when the data is complete and fresh.
- **Unknown or unregistered sessions stay visible** under `_unplaced/` with a reason. Their ticket and parent are never guessed. You act on them only through the choices offered.
- **Archived is a lifecycle state, never an outcome.** Words such as "merged" or "done" appear only with verified evidence.
- **Status never relies on colour alone.** Every status shows a word, a glyph and a shape.

Example (invented):

```text
> ls -l /factories/example-notes          (the CLI's program name is not decided)
ls · complete · fresh 6s ago
[~] building   example-notes/v1/spec-1/T3@r7   loop 2/3 · review   Next: wait for review
[!] needs_you  example-notes/v1/spec-1/T4@r2   plan approval       Next: plan …/T4@r2
```

## Screens

The web client is designed phone-first, with the same features planned on desktop.

| Screen | Reads | Acts |
| --- | --- | --- |
| **Home** | `ls /` | Not yet decided |
| **Needs you** | `next`, `blockers`, pending decisions | Decision types, `launch`, Open links |
| **Factories** | Factory rows (`ls -l /factories`); a two-level live graph (factory → tickets; ticket → its build, test, review and rework loop) | `park`, `resume`, `launch`, decisions on stopped items |
| **Agents** | Connected personal agents only (`ls /agents`); identity, capabilities, permissions and recent requests (`show`) | None in the first version: read-only |
| **Memory** | Domains (`ls /memory`); each domain's items (`ls -l`); one item's provenance and past versions (`show`, `diff`) | None in the first version: read-only. Changes go through the personal agent and the controller's write path |

Web stays live by re-running `ls` every few seconds while a screen is visible. There is no push channel in the first version.

## Visual style (decided)

- Dark first.
- One colour job each: one colour means "you're needed", another means "live or current", and greys do everything else. No red or green.
- Status is a word, a glyph and a shape, checked in greyscale.
- Geist and Geist Mono fonts, bundled with the app.
- A strict 4-point spacing scale, with 44-point touch targets.

## Still open

- Friendlier names for `launch` and `resume`.
- Exactly how the detail levels and the `ls` loop are implemented.
- Home screen content, recurring loops and routines, and rules.
- How memory domains are organised: left to the separate memory design session (see [overview](overview.md#memory-direction-only)).
