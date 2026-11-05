# Fleet settings

Every fleet in this project starts from these settings. The fleet skills read this file instead of naming any project's agents, commands or rules. Each value names its source; when a source changes, run the `setup` skill again (or edit the line by hand) so the two agree.

## Builder

One per ticket. It builds its ticket on its own branch, and never reviews or merges.

| Setting | Value | `room-cli thread spawn` flag |
| --- | --- | --- |
| Harness | <harness, for example Claude Code> | `--provider <id>` |
| Model | <model, for example Opus 5.5> | `--model <id>` |
| Effort | <effort, for example XHigh> | `--reasoning-level <level>` |

Source: <the user's words and the date, or the file that records the choice>.

## Reviewer

One per pull request, from a different model family than the builder. It reviews the exact head and never edits. If it can't be started, the review waits: it is never swapped for another model.

| Setting | Value | `room-cli thread spawn` flag |
| --- | --- | --- |
| Harness | <harness, for example Codex> | `--provider <id>` |
| Model | <model, for example Sol 6.1> | `--model <id>` |
| Effort | <effort, for example Medium> | `--reasoning-level <level>` |

Source: <the user's words and the date, or the file that records the choice>.

## Labels

The triage labels and their names here are in `triage-labels.md`. A fleet adds one label of its own:

| Label | Means | Put on by | Taken off by |
| --- | --- | --- | --- |
| `held` | The ticket is held: it waits for the user's approval of a changed plan or contract, and never starts while it has this label. It has no ready label while held | `plan-graph`, before it changes an approved plan; `launch-fleet`, when a merge changes a contract the ticket relies on | `plan-graph`, as it puts the ready label back once the new approval is recorded; `launch-fleet`'s `release` line, if it is still on after that |

The skill that holds a ticket first creates the label, once, with the description "Held: waits for the user's approval".

Source: the fleet skills, `plan-graph` and `launch-fleet`, which put it on and take it off. It is the same in every project.

## Test commands

The full set, in this order. A builder runs all of it locally, and opens its pull request only when every command passes. <Toolchain versions, as the instructions file gives them.>

| # | Folder | Command | CI check |
| --- | --- | --- | --- |
| 1 | <root, or `<folder>`> | `<command, exactly as the source spells it>` | `<the check that runs it>` |

Source: <the instructions file's commands, linked>, and <the CI workflow, linked>.

## CI checks

| Check | Runs | Required on |
| --- | --- | --- |
| `<job name>` | <the test commands it runs, by number> | `<branch>` |

<What the user should know about these checks: how a draft or a docs-only change shows, for example.>

Source: <the CI workflow, linked>, and the branch rules (`gh api 'repos/{owner}/{repo}/rulesets'`).

## Standing merge rules

| Pull request | Review | Merged by, and when |
| --- | --- | --- |
| <kind of pull request> | <who reviews it, or none> | <who merges it, and the exact conditions> |

<With no rule recorded: "None yet: every merge waits for the user.">

Source: <the user's words and the date, or the files that record each rule>.
