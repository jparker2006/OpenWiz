---
name: launch-fleet
description: Launch a project's fleets and keep them moving. Create the project's five-minute loop when the user approves a plan graph; on each loop tick, start a builder on every ready ticket at once, send green pull requests to their reviewer and findings to their builder, merge approved work, hold tickets whose contracts changed, and archive finished agents. Use when a plan graph is approved, when a loop tick wakes you with due lines, or to see what a fleet has due.
---

# Launch fleet

A **fleet** is the agents working one approved plan graph. This skill starts every ticket that is ready, all at once, and then moves each one through build, review and merge until its spec is done. It works across every approved graph in the project at the same time, whatever their versions.

**Everything it knows comes from GitHub.** A five-minute loop runs apart from the orchestrator, in a fresh thread each time, and finds all its work on GitHub: the plan graph approvals, the tickets, the pull requests, the review reports, and its own records. It never relies on a "finished" message from an agent, or on anyone's memory. The orchestrator only grills, plans and talks to the user.

| Who | Does |
| --- | --- |
| The orchestrator | Creates the project's loop once, and starts a tick when the user approves a plan graph (step 1) |
| The loop's check (`scripts/fleet.sh`) | Every five minutes, reads GitHub and prints one line per thing that is due. When nothing is due it prints nothing, and no agent wakes |
| A tick (a fresh agent the check wakes) | Does each due line, records what it did, and archives itself (step 2) |

The project's settings are in `docs/agents/`: the builder, the reviewer, the CI checks and the merge rules in `fleet.md`, the labels in `triage-labels.md`. Read them; never guess them. If they are missing, run the `setup` skill first.

## The frontier

The **frontier** is every ticket that may start now. A ticket is on it when all of these hold:

- **Approved.** Its row in its spec's latest plan graph approval (the `plan-graph` skill's comment on the spec) says `ready-for-agent`, and that approval is **current**: `plan-graph`'s "Is an approval current?" check passes. The check runs plan-graph's `revision.sh` for the spec, and the SHA-256 of what it prints must be the approval's `Fingerprint:`. Any change to the plan since (the spec's body; a ticket's title, body or arrows; a ticket added or dropped) makes it stale, and then nothing under that spec starts until the user approves the plan as it now stands. Labels, comments and merges never make it stale. If the check can't be done, nothing starts. An issue outside any spec is approved only by the user's approval of it on its own (the `triage` skill's comment: `**Approved on its own by <user>, <date>:**`). A label alone approves nothing, and a person's ticket (`ready-for-human`) is never started.
- **Ready.** It is open and has the `ready-for-agent` label, and no other triage label (`needs-info`, `needs-triage`, `ready-for-human`, `wontfix`). These are the labels' roles; their names in this project are in `triage-labels.md`.
- **Unblocked.** Every ticket blocking it has a **verified merge**: it is closed, and a pull request that closes it merged into the default branch. A closed issue alone is not enough. The blocker can be in any spec and any version: a ticket of the next version starts the moment the exact tickets it needs have merged.
- **Not started.** No builder is recorded on it, it has no open pull request, and it is not held: it has no `held` label (the fleet contract's mark of a held ticket), and no hold of launch-fleet's is waiting for a new approval.

There is no cap. Every ticket on the frontier starts in the same tick.

## 1. Launch (the orchestrator, when the user approves a plan graph)

1. **Check the approval is recorded** on the spec issue, as `plan-graph` writes it. No record, no fleet.
2. **Make sure the project has its loop.** One loop per project drives every fleet in it. `room-cli loop list` shows it if it exists. If not, create it:

   ```sh
   { printf '#!/bin/sh\nGH_REPO=%s\n' "<owner>/<repo>"
     tail -n +2 skills/launch-fleet/scripts/loop.sh; } > /tmp/launch-fleet-loop.sh
   room-cli loop create --name "launch-fleet <owner>/<repo>" --cron '*/5 * * * *' \
     --script-file /tmp/launch-fleet-loop.sh --priority 2 \
     --prompt "You are a launch-fleet tick for <owner>/<repo>. Read skills/launch-fleet/SKILL.md and do step 2 for the due lines below. The orchestrator's thread is <your thread id>."
   ```

   The loop's script (`scripts/loop.sh`) runs first, in the loop's own sandbox, which has `gh`, `git` and `jq` but no working `room-cli`; that is why the check reads only GitHub. The sandbox lasts between runs, so the script keeps a clone of the project there, brings it up to the default branch, and runs the check from it, next to `plan-graph`'s `revision.sh`. Each run that prints something starts a fresh thread. Write the loop in the handoff file (the `handoff` skill). When a new orchestrator takes over, it updates the thread id in the prompt: `room-cli loop update <id> --prompt "..."`.
3. **Start a tick now,** so the fleet launches at once instead of within five minutes: `room-cli loop run <id>`.

Done when the loop exists and a tick has run since the approval.

## 2. Each tick

You were woken because the check printed due lines; they are at the end of your prompt. The world may have moved since, so run the check again in the repository and work from its output:

```sh
sh skills/launch-fleet/scripts/fleet.sh
```

Do every line. Lines of the same kind run **in parallel** (every `start`, every `review`): never one after another. The lines and what each needs:

| Line | Do |
| --- | --- |
| `start #N` | Start its builder (step 3) |
| `reconcile start #N` | A start claimed 10 minutes ago never recorded how it went. Find out (step 3) |
| `check start #N` | Check the builder is really at work (step 3) |
| `review #P for #N at <commit>` | Start the reviewer `fleet.md` names (step 4) |
| `re-check #P ... tell its reviewer, thread <id>` | Ask the same reviewer to re-check the new head (step 4) |
| `reconcile review #P ...` | A reviewer start claimed 10 minutes ago never recorded how it went. Find out (step 4) |
| `check review #P ...` | Its reviewer has posted no verdict on the head 30 minutes after it was started or asked to re-check, or 30 minutes after it was told again. Tell it again, or give up on it (step 4) |
| `CI failed #P ...` | Tell the builder CI failed on that commit, and to fix it and hand back a new green head. It is not a review round |
| `findings #P ...` | Send the reviewer's findings to the builder (step 4) |
| `merge #P for #N at <commit>` | Merge it (step 4) |
| `archive #N` | Archive the agents it names (`room-cli thread archive <id>` for each; one already archived, like a reviewer given up on, is done), then record `launch-fleet: archived` on the ticket, as its last comment. The check keeps finding a closed ticket until it is archived, and closes no spec before then |
| `hold #N` / `note #N` | A contract it relies on changed (step 5) |
| `release #N` | The user approved it again since launch-fleet's own hold: take its `held` label off (`gh issue edit <N> --remove-label held`). It starts on a later line. Only launch-fleet's holds are released here, each once: a `held` label put on after launch-fleet's hold (by `plan-graph`, which takes its own off) is never launch-fleet's |
| `needs-info #N` | Its builder asked a question. Tell the orchestrator, with a link to the question, and record `launch-fleet: asked the orchestrator` |
| `person #N` | A person's ticket (a live step) is ready: approved, unblocked and not held. Tell the user (step 6), and record `launch-fleet: told the user` |
| `re-approve #S` | A ticket could start, but the plan changed since the user approved it, so nothing under the spec starts. Tell the user (step 6) |
| `spec done #S` | Every approved ticket is closed. Write the closing summary (step 6) |
| `close spec #S` | Close the spec issue, with a comment saying every ticket is closed |
| `look #N` | The check can't read something in full, or found two of something, so it leaves that item alone. Tell the orchestrator what the line says, and record `launch-fleet: look: <the reason>` on #N |

**Record each thing you do** on GitHub, with the exact line from "Records" below, so the next tick knows it is done: GitHub is the loop's only memory.

**After a merge,** run the check once more and start whatever the merge unlocked, in this same tick.

**Finish:** `room-cli loop report --outcome acted` (or `needs-you`), with one `--action` per change. Then archive your own thread (`room-cli thread archive --self`), unless something you raised needs the user: then leave it open with a one-line verdict.

Done when every line is done and recorded, and the check prints no line you could act on.

## 3. Builders

**Starting one** (a `start` line):

1. **Claim it.** Comment `launch-fleet: starting` on the ticket, and note your comment's id (the number after `#issuecomment-` in the address `gh issue comment` prints). Then ask the check whether your claim stands: `sh skills/launch-fleet/scripts/fleet.sh claim <N> <your comment's id>`. It reads the ticket afresh and prints `go #<N>` only when no older claim is still live (`starting`, `started` or `start checked`), no builder is recorded, no pull request is open, and the ticket is still on the frontier. Anything else (`withdraw #<N>: <why>`, or an error): edit your claim to `launch-fleet: start withdrawn`, and stop. Two ticks that overlap can't both start a builder: the older claim wins, and it keeps the ticket as you edit it to `started` and `start checked`.
2. **Start it,** with the builder `fleet.md` names, the ticket in its title, and this one-line brief, the same for every builder:

   ```sh
   room-cli thread spawn --top-level --provider <harness> --model <model> --reasoning-level <effort> \
     --title "#<N>: <ticket title>" \
     --prompt "You are the builder for #<N>. Read skills/build-ticket/SKILL.md and follow it."
   ```

   Everything about the work is in the ticket and its spec, so the brief never drifts from them. A top-level thread starts from the default branch on GitHub, and outlives this tick.
3. **Record the outcome** by editing your claim (`gh api --method PATCH repos/<owner>/<repo>/issues/comments/<id> -f body='...'`): `launch-fleet: started, thread <id>` when CloudRoom started it, or `launch-fleet: start refused: <its reason>` when it said no. A refused start stays on the frontier, and the next tick tries again. If the start timed out or you can't tell, leave the claim as it is: the check asks for a reconcile after 10 minutes.

**Reconciling** (a `reconcile start` line): before any retry, find out whether a builder started. Look for a thread titled `#<N>:` in `room-cli thread list`, and for its branch (`git ls-remote origin 'refs/heads/room/<N>-*'`). If one is there, edit the claim to `launch-fleet: started, thread <id>`. If not, edit it to `launch-fleet: start failed: <why>`, and the next tick starts it again. A ticket never gets two builders.

**Checking** (a `check start` line): its thread exists and is working, its title names the ticket, and its branch shows up. Then edit the claim to `launch-fleet: start checked, thread <id>, branch <branch>`. If the thread is gone or never started work, edit it to `launch-fleet: start failed: <why>`.

A builder stays with its ticket through every review round, and is archived when its ticket closes. It is never given another ticket.

## 4. Reviews and merges

A pull request is the loop's when it closes an approved ticket (its description has `Closes #<N>`) and targets the default branch. The builder hands back by pushing a commit whose subject says **green**: the check finds it there, never in a message.

**Claim it first,** for a review or a re-check alike: comment `launch-fleet: review starting <commit>` on the pull request, then run `sh skills/launch-fleet/scripts/fleet.sh claim <P> <your comment's id>`. Go on only on `go #<P>`: no older claim for that commit is live (`review starting`, `review started`, `re-check sent`), the head is still that commit, and it is still due. Otherwise edit your claim to `launch-fleet: review withdrawn <commit>`, and stop.

**Starting a review** (a `review` line, printed only once CI is green on that exact commit): start the reviewer `fleet.md` names, as in step 3, titled `Review #<P> at <short commit>`, with the brief `You are the reviewer for pull request #<P> at <commit>. Read skills/review-ticket/SKILL.md and follow it.` Then edit the claim to `launch-fleet: review started <commit>, thread <id>`. If CloudRoom can't start that reviewer, edit it to `launch-fleet: review refused <commit>: <reason>`: the review waits, and is never given to another model.

**A re-check** (a `re-check` line) goes to the same reviewer: `room-cli thread tell <its thread> "Re-check pull request #<P> at <commit>: read skills/review-ticket/SKILL.md, Re-checks."`, then edit the claim to `launch-fleet: re-check sent <commit>, thread <id>`. When the builder merges `main` into approved work, its new head is re-checked the same way. A `reconcile review` line means a claim never recorded how it went: ask the reviewer's thread whether it was started or told, and edit the claim to match, or to `launch-fleet: review failed <commit>: <why>`.

**A quiet reviewer** (a `check review` line): no `VERDICT` names the head 30 minutes after its reviewer was started or asked to re-check it. The line names the reviewer's thread and the comment that records it. The reviewer gets one more try, then a fresh reviewer takes over:

1. **No verdict 30 minutes after the record:** read the thread (`room-cli thread output <its thread> --all`). A tick can read and tell only the threads it started itself, so a reviewer started by an earlier tick or by the orchestrator is out of reach. When the thread is out of reach or gone, do step 2 now. Otherwise tell it again, once: `room-cli thread tell <its thread> "No report yet for pull request #<P> at <commit>. If you are still reviewing, finish and post it. If not, start now: <the brief>"`, with the brief it was given (the review brief above for `review started`, the re-check message for `re-check sent`). Then record `launch-fleet: review told again <commit>, thread <id>` as a new comment on the pull request.
2. **Still no verdict 30 minutes after it was told again,** or the thread is out of reach: archive the thread (`room-cli thread archive <its thread>`), so it can't post a late verdict. Then edit the comment the line names (`gh api --method PATCH repos/<owner>/<repo>/issues/comments/<id> -f body='...'`) to `launch-fleet: review failed <commit>, thread <id>: <why>`. If the archive failed, say so in `<why>`: the `archive` line lists the thread again when the ticket closes.
3. **The next check prints** `review #<P> for #<N> at <commit>: a fresh reviewer`: start one as in "Starting a review", the reviewer `fleet.md` names, never another model. A thread recorded as `review failed` is never told again or asked to re-check.

**The report** is the latest comment on the pull request that starts `VERDICT: APPROVE at <commit>` or `VERDICT: CHANGES at <commit>`, with the full 40-character commit. It counts only for the commit it names, and only an `APPROVE` naming the current head approves. No report, a timeout, an error or a report on an older commit is never approval.

**Findings** (a `findings` line): tell the builder `The review of pull request #<P> at <commit> asks for changes: read the report there. Fix or dispute each open finding, then hand back a new green head.`, and record `launch-fleet: findings sent <commit>`. Then read the report's `Next:` line. Each finding gets two correction rounds, then the orchestrator's diagnosis, then one more round, then the user decides: when `Next:` asks for the orchestrator's diagnosis, tell the orchestrator; when it says the user decides, tell the user (step 6).

**Merging** (a `merge` line):

1. **Check the verdict came from the reviewer.** Every agent posts from the same GitHub account, so the comment alone can't show who wrote it. Read the reviewer's own thread (`room-cli thread output <its thread> --all`, the thread named in the `review started` record): it must show the same `VERDICT: APPROVE at <commit>` line, which `review-ticket` has the reviewer read back after posting. If it doesn't, don't merge.
2. **Check the merge rules** in `fleet.md` for this commit: every required CI check green on it, and the pull request's base unchanged.
3. **Merge that exact commit,** naming what allowed it: `gh pr merge <P> --merge --match-head-commit <commit> --body "<the APPROVE and the plan graph approval it relies on>"`.
4. **Read it back:** `gh pr view <P> --json state,mergeCommit,headRefOid` shows `MERGED` and the approved commit. Only this is a verified merge. The ticket closes through its `Closes #<N>` line; close it yourself if it didn't.

If any of this fails, record `launch-fleet: merge blocked <commit>: <why>` on the pull request. When `main` moved and the branch conflicts, tell the builder to merge `main` in (never rebase or force-push); its new head goes back for a re-check. Otherwise tell the orchestrator.

## 5. Contracts

A ticket lists the contract files it relies on in its "Relies on" line. On every merge into the default branch, the check looks at the files the merge changed. A file that a merge **creates** is the planned delivery of a contract, not a change. A file it edits, renames or deletes is a change.

- **`hold #N`** (no builder yet, or a person's ticket): put the `held` label on it and take its ready label off, `ready-for-agent`, or `ready-for-human` for a person's ticket (`gh issue edit <N> --add-label held --remove-label ready-for-agent`; if the project has no `held` label yet, create it once first: `gh label create held --description "Held: waits for the user's approval"`). Then, after the labels, comment `launch-fleet: held for #<P>: contract changed, re-approve. <the files> changed in #<P>.`: that order is what marks the `held` label as launch-fleet's. Tell the user (step 6). The user re-approves just that ticket, through `plan-graph`'s next revision. Only once that approval lands does the check print `release #N`, and the `held` label comes off.
- **`note #N`** (being built): tell its builder which files changed in which pull request, and to check its work against them. Record `launch-fleet: noted #<P>`.

## 6. Telling the user

The fleet talks to the user only when it needs them: a contract hold, a plan that changed since its approval, a finding still open after its last round, a live step (a `person` line), and once when a spec closes. Everything else stays quiet; the user can always ask the orchestrator where things stand. Each one goes to two places: a message in the orchestrator's thread (`room-cli thread tell <the orchestrator's thread> "Needs you: ..."`), and a comment on the spec issue starting `**Needs you:**` (on the issue itself, for one approved on its own). The user reads and answers it in the orchestrator's thread, on the web page.

- **`re-approve #S`:** say that the plan changed since which revision (compare `revision.sh`'s output with the approval's revision text to see what), and record `launch-fleet: needs approval: <the line's text after "re-approve #S: ">`. Nothing under the spec starts until a new revision approves it. Tickets already being built keep going.
- **`spec done #S`:** the closing summary, as a comment on the spec that starts `launch-fleet: spec summary`: what merged, and the follow-ups (low review findings) for the user to approve or drop in one go. The line names them: open sub-issues of the spec outside its plan, and open issues whose body starts `Follow-up of #<ticket> (spec #S)`. The spec closes only once they are all closed too. If the spec was its version's last, add that the version is ready for its release branch: code reaches `main` as soon as it is reviewed, but release branches and live checks go in version order, and both are the user's. Tell the orchestrator too.

## Records

The loop's memory: comments on GitHub, each starting `launch-fleet:`. Edit a claim as its start moves on; a retry is a new comment.

| On | Record | Means |
| --- | --- | --- |
| A ticket | `launch-fleet: starting` | A start is claimed |
| | `launch-fleet: started, thread <id>` | Its builder started |
| | `launch-fleet: start checked, thread <id>, branch <branch>` | Thread, ticket and branch checked |
| | `launch-fleet: start refused: <reason>` / `start failed: <why>` / `start withdrawn` | No builder: it stays on the frontier |
| | `launch-fleet: held for #<P>` / `noted #<P>` | A contract hold, or a builder told of a change |
| | `launch-fleet: asked the orchestrator` | Its `needs-info` question was passed on |
| | `launch-fleet: told the user` | A person's ticket is ready, and the user knows |
| | `launch-fleet: archived` | Its builder and reviewer are archived |
| A pull request | `launch-fleet: review starting <commit>` / `review started <commit>, thread <id>` / `review refused <commit>: <reason>` / `review failed <commit>: <why>` / `review withdrawn <commit>` | Its reviewer for that commit |
| | `launch-fleet: re-check sent <commit>, thread <id>` | The same reviewer asked to re-check |
| | `launch-fleet: review told again <commit>, thread <id>` / `review failed <commit>, thread <id>: <why>` | A quiet reviewer was told again, or given up on (its thread archived) |
| | `launch-fleet: CI failure sent <commit>` / `findings sent <commit>` | The builder was told |
| | `launch-fleet: merge blocked <commit>: <why>` | The merge waits for someone |
| A spec | `launch-fleet: needs approval: <what changed>` / `spec summary` | The user was asked, or told it is done |
| Any of them | `launch-fleet: look: <the line's reason>` | Someone was asked to look |

## The check

`scripts/fleet.sh`, beside this file, is the loop's check, and the frontier and contract-hold helpers. It only reads, through `gh` and `jq`. For a spec with a ticket that could start, it runs `plan-graph`'s `revision.sh` (in `skills/plan-graph/`) to check the approval is current. `scripts/loop.sh` is the loop's script, which runs it.

| Command | Prints |
| --- | --- |
| `sh scripts/fleet.sh` | Every due line, or nothing when nothing is due (exit 0) |
| `sh scripts/fleet.sh frontier` | Every ticket that may start now |
| `sh scripts/fleet.sh holds` | Every contract hold and builder note due |
| `sh scripts/fleet.sh claim <N> <comment id>` | After a claim on ticket or pull request `<N>`: `go #<N>`, or `withdraw #<N>: <why>` |

It exits 2, with the reason, when it can't read what it needs. Its tests run on synthetic GitHub answers: `sh scripts/test.sh`.

## Credit

The frontier of a task graph, a fresh agent for each ticket started as soon as the frontier allows, and briefs that point at the spec and the ticket instead of copying them, are adapted from Matt Pocock's `implement-spec` skill (`github.com/mattpocock/skills` @ `a496f55`), which carries this notice:

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
