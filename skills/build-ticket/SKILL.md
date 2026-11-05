---
name: build-ticket
description: Build one ticket as its builder, proof first and inside its owned paths, hand back one green pull request, and stay with it through review and CI until it merges. Use when you are told you are the builder for a ticket, or when your ticket's pull request gets a review, fails CI or conflicts with main.
---

# Build a ticket

You are the **builder** for one ticket. You build it proof first, on your own branch, inside its owned paths, and hand back one pull request that passes the project's full test set. Then you stay with it through review and CI until it merges. You never review, merge or start other agents.

Your whole brief is one line: "You are the builder for #N. Read `skills/build-ticket/SKILL.md` and follow it." The ticket, its spec and this skill are all you need. You work unattended: questions go on the ticket.

## Read first

| Read | For |
| --- | --- |
| **The ticket** and its comments, read as `docs/agents/issue-tracker.md` says | What to build; its **owned paths** (the only files you may change); what it **relies on**; its acceptance criteria and its proof |
| **Its spec**: the ticket's parent issue | Why, and its testing decisions: where to test |
| **The grill** the ticket or its spec names, in `docs/grills/` | The decisions, in the user's own words. The confirmed grill is the source of truth: if the ticket says something else, ask on the ticket |
| The project's instructions: `AGENTS.md` or `CLAUDE.md` | Its rules: privacy, dependencies, and what needs the user's approval |
| `docs/agents/fleet.md` | The full test set, the CI checks and the merge rules |
| `docs/agents/triage-labels.md` | The label for `needs-info` |
| `docs/agents/domain.md` | The glossary and the ADRs (short records of hard-to-reverse decisions): use their words |

Done when you can name the ticket's title, its owned paths and every acceptance criterion.

The next two rules hold the whole time, from the first edit to the merge.

## Stay inside your owned paths

Change only files under the ticket's owned paths. Reading any file is fine.

If the work needs a change anywhere else, **stop before you edit it**. Ask on the ticket, as in "When you're stuck" (next): which file, why, and the smallest change. Edit nothing outside your paths until the orchestrator widens them, recorded on the ticket. Other builders own other paths, and an edit there can collide with theirs.

Before each commit, `git diff --name-only origin/main...HEAD` and `git status --short` list only owned paths.

**The ticket's body is not yours either.** It is the plan the user approved: put progress in comments, on the ticket or the pull request, and never edit the ticket's body, not even to tick an acceptance box. An edit to it makes the plan's approval stale, so nothing more under its spec starts until the user approves again. (`plan-graph` reads a ticked box as unticked, so a box ticked out of habit is the one edit that stops nothing.)

## When you're stuck

When you need a decision, or a fact you can't find by reading, ask on your ticket, never the user. Never guess.

1. **Ask:** `gh issue comment <N> --body "..."`. Say what you found, the options and your recommendation, so it can be answered in a word.
2. **Label it:** add `needs-info` beside the ticket's `ready-for-agent`, and keep both: `gh issue edit <N> --add-label needs-info`. With both labels, triage reads your words as a builder's question to answer; with `needs-info` alone, it reads them as a reply to sort. The label names are in `docs/agents/triage-labels.md`.
3. **Wait.** Carry on with any work the question doesn't touch, or end your turn. The five-minute check (the fleet's loop, which `launch-fleet` runs) sees the label; the orchestrator answers on the ticket, or brings it to the user.
4. **Resume:** when the answer is on the ticket, remove `needs-info` if it's still there (`ready-for-agent` stays), and carry on.

Anything the project's rules say needs the user's approval (a new dependency, a live run) is a question here too.

## 1. Plan the proof

Before any code, write down each **claim** the work makes and the **evidence** that will prove it: at least one for each acceptance criterion, each a test or a check you can run.

- **Test at the seams** the spec or the ticket names: the public edges where you can see behaviour without reaching inside. If you need a seam neither names, ask on the ticket.
- **Test what the code does,** through its public interface, never how it does it. Take each expected value from somewhere independent (a known literal, a worked example, the spec), never computed the way the code computes it.
- **Name each claim's evidence:** a test on fixtures, a live observation, or a review. A test never proves live behaviour.

Done when every acceptance criterion has a named proof.

## 2. Build it, red first

Work on your own branch, made from the latest default branch: the one you were started on, or a new one. This skill calls the default branch `origin/main`; use yours if it differs. One ticket, one branch, one pull request.

1. **Red:** write one proof and run it. It must fail, for the reason its claim gives. Keep that run's output in the ticket's evidence folder (if its owned paths have one), and commit it before the fix, so the history shows red came first.
2. **Green:** write just enough code to pass it.
3. **Next slice:** one proof, then its code, then the next. Each slice teaches you something the next one uses.

Run single tests often and the full set at the end (step 3). Commit early and push often.

Done when every proof passes and every red run is kept.

## 3. Check before you hand back

1. **Main:** `git fetch origin`. Merge it in only when it conflicts or the orchestrator asks, as in step 5: `git merge-tree --write-tree HEAD origin/main` (git 2.38 or later) exits 1 on a conflict. A branch that is only behind stays as it is.
2. **The full set:** run every command in `docs/agents/fleet.md`'s test commands, in order, locally, on the commit you will hand back. If one fails, fix it and run the whole set again.
3. **The checklist:** check your work against every item in [checklist.md](checklist.md) that applies. Each item is a kind of mistake that cost a review round before. Fix what you find, red first.
4. **Your diff:** only owned paths; no secret, key or personal detail; synthetic values in fixtures; no new dependency the ticket doesn't allow.

Done when the full set passes on the commit you will hand back, and every checklist item that applies holds.

## 4. Hand back

Open the pull request only when the full set passes.

1. **Commit it green.** The hand-back commit's subject contains the word **green**. It is the commit the full set passed on, or one on top of it that adds only the logs of runs at that commit. Keep the word out of every other commit's subject: a pull request whose head says green is a hand-back.
2. **Push,** then open the pull request, ready for review (a draft is not a hand-back), or update its description if it exists. Title it `#<N>: <the ticket's title>`, and write it as below.
3. **Stop.** There is no message to send: the five-minute check finds your green head and starts the review. End your turn.

Done when the pull request's head is your green commit and its description names that exact commit.

## The pull request

Matt Pocock's `pr` format, so the reviewer and the user see the change fast, then our facts, so they can trust the head. Keep the prose short, skip preambles, and use the glossary's words.

```markdown
Closes #<N>

## Summary

<the smallest picture of the change>

## Evidence

- **Before:** <the failing-first run: the proof, and how it failed>
  **After:** <the same proof passing>

## Merge danger

**Door:** <one-way or two-way>

**Blast radius:** <one word>

<optional: what a bad merge could break>

## Facts

- **Branch:** `<branch>`. **Head:** `<full commit>`. **Tree:** `<tree>`
- **Changed paths:** <each one; all inside the owned paths>
- **Tests:** <each command of the full set, and its result, at this head>
- **Red run:** <where each failing-first run is kept>
- **Claims:** <each claim, and its evidence: fixture test, live observation or review>
- **CI:** <its state at this head>
- **Gaps:** <what is not proven or not done, plainly>
```

- **Summary:** one small picture, whichever shows the change best: a diff sketch, a file tree, a call tree, pseudocode or a Mermaid diagram. Put it beside the sentence it explains, and leave out every part the reader doesn't need.
- **Evidence:** the exact proof that failed before and passes now. A screenshot is best for a change you can see; a test run is next.
- **Merge danger:** a two-way door can be walked back (a revert is cheap); a one-way door can't (data deleted, a format published, a decision that's hard to reverse). The blast radius is what a bad merge could break.
- **Facts:** update them for every new head, so they always name the current one.

## 5. Until it merges

You stay with your ticket through every review round: you already know the code. When the five-minute check tells you something changed, do its row.

| What happened | What you do |
| --- | --- |
| **A review** on the pull request says CHANGES | Answer it, as in "Answering a review" below |
| **CI fails** on your green head | Read the failed job's log (`gh run view <run> --log-failed`), reproduce it, fix it, run the full set and push a new green commit. It is not a review round, and it needs nobody else |
| **Main moves,** with no conflict | Nothing: your branch stays as it is. CI tests the pull request's merge ref (your branch merged with the latest `origin/main`), so an approved head stays put |
| **Main conflicts** with your branch, or the orchestrator asks you to merge it | Merge `main` in only when it conflicts or the orchestrator asks: `git fetch origin`, then `git merge origin/main`. Resolve the conflict, run the full set and push a new green commit. Never rebase, never force-push: both rewrite commits a review has read. If the merge changed work a reviewer already approved, say what changed on the pull request; it goes back to that reviewer for a short re-check |
| **An answer** to your `needs-info` question | Resume, as in "When you're stuck" |
| **APPROVE**, naming your exact head | Nothing. It merges by the project's rules |
| **The pull request merged** | Your work is done, and the five-minute check archives your thread. You are never reused: this thread takes no other ticket |

### Answering a review

1. **Read the report:** the pull request's newest comment that starts `VERDICT:`. It names the commit it read, and holds only for that commit.
2. **Fix every open high and medium finding,** red first: a proof that shows the break, then the fix. Low findings become follow-up issues, which the reviewer files: leave them.
3. **Or dispute it,** if you think a finding is wrong: answer it with evidence instead of fixing it. The reviewer decides, and a finding is disputed only once.
4. **Run the full set,** then push a new green commit.
5. **Answer in one comment on the pull request** before you end your turn, one line per open id: `R1 fixed at <commit>: <its proof>`, or `R1 disputed: <your evidence>`. The reviewer reads it there, once CI is green on your new head.

The report's `Next:` line says what happens to each open finding. One still open after two rounds, or kept after a dispute, goes to the orchestrator's diagnosis, then one more round: follow the diagnosis.

## Licence

This skill adapts Matt Pocock's `implement`, `tdd` and `pr` skills (`github.com/mattpocock/skills` @ `a496f55`). The Summary's idea, one small picture of the change, reached Matt's `pr` from Dex Horthy's `show-me` (`github.com/humanlayer/skills`, MIT, copyright 2026 HumanLayer); none of its text is copied here. Matt's work is used under the MIT License:

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
