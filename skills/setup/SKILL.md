---
name: setup
description: Set up a project for fleets, writing its issue tracker, triage labels, glossary layout and fleet defaults (builder, reviewer, test commands, CI checks, merge rules) to docs/agents/. Use before a project's first fleet, when a skill finds docs/agents/ missing, or after one of those settings changes.
---

# Setup

Write a project's settings once, as four short files in `docs/agents/`. Every other fleet skill reads these files, so no skill is tied to one project. Run it as the project's orchestrator, before its first fleet.

Every value comes from something you read in the project or something the user decided, and is written down with that source. So a second run, with nothing new, changes nothing.

| File | What it says |
| --- | --- |
| `docs/agents/issue-tracker.md` | Where issues live (GitHub), and where versions, specs and tickets sit there |
| `docs/agents/triage-labels.md` | The five triage labels, and what each one means |
| `docs/agents/domain.md` | Where the glossary, the ADRs (short records of hard-to-reverse decisions) and the grills live, and how to read them |
| `docs/agents/fleet.md` | The default builder and reviewer, the test commands, the CI check names and the standing merge rules |

Each file starts from its template beside this skill: [issue-tracker.md](templates/issue-tracker.md), [triage-labels.md](templates/triage-labels.md), [domain.md](templates/domain.md) and [fleet.md](templates/fleet.md).

## 1. Read what is there

Read; never assume. This step is done when you can name, for every value in the four files, the file, command or decision it comes from, or that nothing in the project says.

- **The repository.** `gh repo view --json nameWithOwner` prints it. If the project is not on GitHub, stop and tell the user: fleets work only with GitHub issues for now.
- **Earlier settings.** If `docs/agents/` already holds these files, this is a re-run: read them first, then go to "Running it again" below.
- **Labels.** `gh label list --limit 500 --json name,description`. Note which of the five triage labels exist, and under what names.
- **Glossary, ADRs and grills.** `GLOSSARY.md` (or `GLOSSARY-MAP.md`) at the root, `docs/adr/` and `docs/grills/`. Note whether the repository holds several separate packages (a `pnpm-workspace.yaml`, a `workspaces` field in `package.json`, or `packages/*` folders with their own `src/`).
- **Builder and reviewer.** Any agent the user already chose for this project's builders or reviewers: in its instructions file (`AGENTS.md` or `CLAUDE.md`), its ADRs, and the comments on its spec issues.
- **Test commands.** The commands in the instructions file, and every `run:` step in `.github/workflows/`.
- **CI checks.** The job names in `.github/workflows/`, and which of them must pass before a merge: `gh api 'repos/{owner}/{repo}/rulesets'`, then each ruleset's required checks (`gh api 'repos/{owner}/{repo}/rulesets/<id>'`), and the default branch's protection (`gh api 'repos/{owner}/{repo}/branches/<branch>/protection'`).
- **Merge rules.** Every rule the user has recorded about who merges what, and when: in the ADRs, the instructions file and the spec issues. Keep the user's exact words.

## 2. Ask only what nothing answers

First tell the user, in a few lines, what step 1 found and what is missing. Then take the settings in order: one setting, one answer, then the next. Lead each with your recommendation, so the user can accept it in a word. Skip any that step 1 already settled.

1. **Issue tracker:** GitHub, this repository. Once `gh repo view` prints it, there is nothing to ask.
2. **Triage labels:** "Keep the five standard triage labels?" (recommended: **yes**). They are `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human` and `wontfix`, each named after its role. If the project already uses another name for a role, map that name to the role, so no role ever gets a second label.
3. **Glossary and ADRs:** one `GLOSSARY.md` and one `docs/adr/` at the root, and grills in `docs/grills/`. Write this without asking. Only if the repository holds several separate packages, offer one glossary per package, listed in a root `GLOSSARY-MAP.md`, and ask which they want. If the project already has a layout, keep it.
4. **Builder and reviewer:** recommend the project's recorded choice. With none, recommend Claude Code, Opus 5.5, XHigh for builders and Codex, Sol 6.1, Medium for reviewers. The reviewer is always from a different model family than the builder.
5. **Test commands and CI checks:** copy them from their sources, exactly. If the project has none, say so; never invent one.
6. **Merge rules:** copy the recorded rules. With none, recommend this one: approving a plan graph lets each of its tickets merge as soon as its reviewer approves that exact commit and CI is green, with the base unchanged and the merge read back; specs, plan graphs, live runs, releases and publishing stay the user's. Write the user's answer word for word, with the date. Until they answer, the rule is: every merge waits for the user.

## 3. Write

Fill each template from what you read in step 1 and the answers in step 2.

- **Sources:** every value in `fleet.md` names its source beside it: a linked file, a command, or the user's words with the date.
- **Labels:** create a label only for a role that has none in the tracker (`gh label create <name> --description "<meaning>"`). A role that already has a label keeps it.
- **Only these files:** write the four files in `docs/agents/` and any missing labels. The other skills read these files by their paths, so nothing else needs to point at them, and no skill file changes.
- **No run date:** the files hold settings, not a record of the run, so an unchanged project gives the same files, byte for byte.
- **One pull request:** commit the four files on a branch and open one pull request. It is the user's chance to read and edit the settings before any fleet relies on them.

This step is done when the four files exist, every value in `fleet.md` matches its source as you read it in this run, and every label in `triage-labels.md` exists in the tracker exactly once.

## Running it again

Read the four files, then read each source again. Change a line only where its source now says something different, or where a value is missing; leave every other line exactly as it is. If nothing changed, write nothing, open no pull request, and tell the user so.

## 4. Tell the user

Say what you wrote, what you asked, and what you left as it was. The user can edit `docs/agents/*.md` by hand at any time. Run this skill again when the tracker, the labels, the agents, the test commands, the CI checks or the merge rules change.

## Credit

This skill adapts Matt Pocock's `setup-matt-pocock-skills`, and its templates adapt that skill's `issue-tracker-github.md`, `triage-labels.md` and `domain.md` (`github.com/mattpocock/skills` @ `a496f55`). `fleet.md` is OpenWiz's own. Matt's work is used under the MIT License:

Copyright (c) 2026 Matt Pocock

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
