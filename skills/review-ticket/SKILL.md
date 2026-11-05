---
name: review-ticket
description: Review a ticket's pull request at its exact head once CI is green, on Standards, Spec and one full sweep, and post the verdict and findings on the pull request. Use when you are a pull request's reviewer, for a re-check of its open findings, or to tell whether a pull request needs a review.
---

# Review a ticket

You review one ticket's pull request. Pin its **exact head** (the commit at its tip when you start), run three checks, **Standards**, **Spec** and one **full sweep**, and post one report on the pull request: the verdict, the commit you read, and every finding with a stable id, a severity and the exact fix.

You read, run and report; the builder changes the code. Your only writes are the report and its follow-up issues.

## Who reviews, and when

The reviewer is the one `docs/agents/fleet.md` names, from a different model family than the builder. There is one reviewer per pull request: the same one does every re-check, since it already knows the code, and it is archived when the pull request merges. It works alone: helper swarms are outside this workflow.

| The pull request | Review |
| --- | --- |
| A ticket's, with CI green on its exact head, whatever it changes: code, a skill, docs, or only evidence | Yes: a full review the first time, then a re-check after each fix |
| A ticket's, with CI still running, or failed, on its head | Not yet. A CI failure goes back to the builder, and is not a review round |
| The orchestrator's docs-only or evidence-only change, outside any ticket: a status page, the handoff file, evidence | No review |
| The orchestrator's grill, once the user has confirmed it, with the glossary terms and ADRs it settled | No review |
| One that needs a review, when the reviewer `fleet.md` names can't be started | The review waits: never another model in its place |

Every ticket's pull request, a skill's included, is always reviewed, as `fleet.md`'s merge rules say: skills are the product, not docs. Only the orchestrator's own changes outside any ticket skip review. For example:

- A ticket's pull request that adds only evidence, such as a red run's log, is reviewed.
- The orchestrator's pull request that updates the handoff file and its evidence, outside any ticket, gets no review.

For a case this table doesn't cover, `fleet.md`'s merge rules decide.

**Only an APPROVE that names the exact head approves.** No report, a timeout, an error, or a report on an older commit is never approval.

## 1. Pin the head

1. **Find the ticket.** The pull request's description has a `Closes #<n>` line. Read the ticket and its spec (the ticket's parent issue): `gh issue view <n> --json title,body,comments,parent`. Read any grill or contract the ticket names.
2. **Pin the head.** Read it once, and use only it from here on: `gh pr view <N> --json headRefOid,baseRefName,isDraft`. Check it out: `git fetch origin <base> pull/<N>/head`, then `git checkout --detach <head>`.
3. **Check CI on that commit:** `gh api 'repos/{owner}/{repo}/commits/<head>/check-runs' --jq '.check_runs[] | {name, status, conclusion}'`. Every check `fleet.md` requires must be `completed` with `success`.

If you have posted a report on this pull request before, this is a re-check: carry on at "Re-checks" below.

**Stop before reviewing, and post nothing,** when you are not the reviewer `fleet.md` names, or the pull request names no ticket, is a draft, needs no review (the table above), or lacks green CI on this exact commit. Say why in your reply.

Done when you hold one head commit with green CI, and have read its ticket and spec.

## 2. Run the three checks

Run each check on its own, and keep its findings apart: a change can follow every rule and still do the wrong thing, or do the right thing and break a rule. Read the diff, `git diff origin/<base>...<head>` (three dots: from where the branch left its base), and the code around it.

**Standards: does it follow the project's rules?** Read the instructions file (`AGENTS.md` or `CLAUDE.md`, and any in the folders the diff touches), `docs/agents/`, and the ADRs (records of hard-to-reverse decisions) for this area. In every project, check:

- **Owned paths:** it changes only the ticket's owned paths, or paths the orchestrator widened on the ticket (`git diff --name-only origin/<base>...<head>`).
- **Proof first:** the failing-first (red) run is kept as evidence, from before the fix.
- **Weakened gates:** see step 3.
- **The project's own rules,** such as: guards fail closed (they refuse whatever they don't recognise), privacy, and which layer may depend on which.

Skip what tooling already enforces: CI passed.

**Spec: does it do what its ticket and spec promise?** First write the ticket's intent in one line, in your own words; every Spec finding points at it. Then look for:

- a promise missing, or only partly kept (quote the ticket's or the spec's line);
- behaviour nobody asked for;
- something that looks done but is wrong;
- proof that doesn't prove: each claim's test must fail without the change and pass with it. Try it: undo the change in a scratch copy, never committed, and run the test.

**Full sweep.** Read the whole diff once more for anything else: bugs, broken callers, error paths, security, privacy. List every finding in one report: a finding held back costs a whole round later.

Done when all three checks have covered the whole diff, and every finding has its file and line, the check that found it, and the exact fix.

## 3. Grade each finding

Grade by impact: what breaks, and for whom, not how sure you feel.

| Severity | Means | Blocks the merge? |
| --- | --- | --- |
| **high** | The ticket's own case is broken | Yes |
| **medium** | A real bug someone will hit | Yes |
| **low** | Everything else, such as wording and tidy-ups | No: it becomes a follow-up issue, and the pull request merges now |

High and medium findings block the merge; low findings never block it.

- **A high finding needs a run.** Give the command or failing test that shows the break, what it printed, and what the ticket needs it to print. Rerun it on a clean checkout of the exact head just before you post. If no run shows the break, the finding is not high: report it as medium, from reading.
- **Medium and low can come from reading,** with the exact file and line.
- **A weakened gate is at least medium.** Any change that relaxes a test, a CI check or a fail-closed guard is at least medium, even when the ticket asked for it. Call it out, and say whether the ticket asked.

Done when every finding has one severity by these rules, and every high finding's command has been rerun at the exact head.

## 4. Post the report

1. **File each new low finding as a follow-up,** so its line can name the issue: a standalone issue labelled for the `needs-triage` role, never a sub-issue of the spec, whose body starts with the line `Follow-up of #<ticket> (spec #<spec>)`. A sub-issue would join the spec's plan graph, and `plan-graph`'s approval covers every sub-issue, so one low finding would make the approval stale and stop every new start under the spec. That first line ties the follow-up to its ticket and spec for triage and the spec's closing summary. The label may have another name, such as `triage: new`, so read it from the table in `docs/agents/triage-labels.md`: the `label=` line prints the "Label here" cell of the `needs-triage` row. Write the body to a file with a quoted heredoc, so the shell runs nothing in the finding's backticks or `$`, and keep backticks, `$` and double quotes out of the short title.

   ```sh
   label=$(awk -F'|' '$2 ~ /^ *`needs-triage` *$/ { l = $3; gsub(/^[ `]+|[ `]+$/, "", l); print l }' docs/agents/triage-labels.md)
   cat > follow-up.md <<'EOF'
   Follow-up of #<ticket> (spec #<spec>)

   <the finding line>. Found in pull request #<N> at <head>; it did not block the merge.
   EOF
   gh issue create --label "$label" --title "<the claim, short> (follow-up to #<ticket>, R<k>)" --body-file follow-up.md
   ```

   If the lookup prints no label, or more than one, stop before filing or posting anything, and say why in your reply: the project's settings need the `setup` skill. The user approves or drops all of a spec's follow-ups together, when the spec closes; one joins a plan only then, as a ticket of a plan graph the user approves, or approved on its own.
2. **Write the report** in the format below.
3. **Post it as a comment on the pull request:** `gh pr comment <N> --body-file <report file>`. It is a comment, not a GitHub review: the account that opened a pull request can't approve its own, and agents often share that account.
4. **Read it back:** `gh pr view <N> --json comments --jq '[.comments[].body | select(startswith("VERDICT: "))] | last'` must print your report. If posting failed, read the comments before posting again, so the report appears once.

The report is the whole hand-back: the builder and the five-minute loop both read it on the pull request, and nobody copies it anywhere.

Done when the report is on the pull request, read back, and starts with the `VERDICT` line for the head you pinned.

## Re-checks

You are started again when the builder has pushed fixes and CI is green on the new head. Pin the new head (step 1). A re-check reads only:

- **the ids still open:** the high and medium findings not yet resolved or withdrawn;
- **the code changed since the commit you last read:** `git diff <last> <head>`. If the builder merged `main` in, judge only what changes this pull request's own work.

Then:

1. **Read the builder's answers,** in its comments on the pull request since your last report. For each open id, the builder either fixed it or disputed it once, with evidence, instead of fixing it.
2. **Give each earlier id its status** (the table under "The report"). For a dispute, you decide: `withdrawn` if the evidence holds; `disputed, kept` if it doesn't, which goes straight to the orchestrator's diagnosis instead of another round. A finding is disputed only once.
3. **Run the three checks on the changed code.** A new finding there takes the next id.
4. **Grade and post** as in steps 3 and 4, in the same format.

**If `main` is merged in after your APPROVE** and it changes what you approved, you are started for a short re-check of that change: your APPROVE held only for the commit it named.

**2 + 1, per finding:** each finding gets 2 correction rounds (one builder fix and one re-check each), then the orchestrator's diagnosis, then one more round, then the user decides. The report's `Next:` line says where each open finding stands.

## The report

Six parts, each its own paragraph, in this order:

```md
VERDICT: <APPROVE or CHANGES> at <the head commit, all 40 characters>

Full review by <harness, model, effort>. CI green on this commit: <the required checks>.

Intent: <the ticket's promise, in one line of your own words>

- **R1 [high]** `<file>:<line>` (<Standards | Spec | Sweep>) <what is wrong, and for whom>. Fix: <the exact fix>.
  Shows the break: `<command>` printed `<its output>`; the ticket needs `<the right output>`. Rerun at this commit: same.
- **R2 [medium]** `<file>:<line>` (<the check>) <what is wrong>. Fix: <the exact fix>.
- **R3 [low]** `<file>:<line>` (<the check>) <what is wrong>. Fix: <the exact fix>. Follow-up: #<issue>.

Checked: Standards (<the rule files read>), Spec (#<ticket>, spec #<spec>), full sweep. Ran: `<command>` (<result>); ...

Next: <who acts on each open finding>.
```

- **The `VERDICT` line** names the commit you read, and holds only for it. `CHANGES` while any high or medium finding is open, otherwise `APPROVE`: low findings never hold it back.
- **The second line,** on a re-check, reads `Re-check <n> of <the open ids> and the code changed since <short commit>, by <harness, model, effort>. CI green on this commit: <the required checks>.`
- **Ids are stable:** R1, R2, … in the order found, never renumbered or reused. A re-check lists every earlier id, in order, and a new finding takes the next number.
- **A new finding** is one line: id, severity, file and line, the check that found it, what is wrong, and the exact fix. A high one has its `Shows the break:` line under it; a low one names its follow-up.
- **No findings:** the line `No findings.` stands in place of the list.
- **The report is as public as the pull request:** point at files and lines, and leave secrets and personal data out.

On a re-check, each earlier id gets one status in place of its finding:

| Status | When |
| --- | --- |
| `resolved at <short commit>: <how you checked>` | It is fixed |
| `withdrawn: <why the builder's evidence holds>` | The builder's dispute convinced you |
| `still open after <k> rounds: <why>` | The fix didn't fix it. `<k>` counts your re-checks that found it open, not the one that kept a dispute |
| `disputed, kept: <why the evidence doesn't hold>` | The dispute didn't convince you |
| `follow-up #<issue>.` | A low finding, already filed |

`Next:` says who acts on each open finding:

| An open finding | Next |
| --- | --- |
| New, or still open after 1 round with no diagnosis yet | The builder fixes it, or disputes it once |
| Still open after 2 rounds, or `disputed, kept` | The orchestrator's diagnosis, then one more round |
| Still open in the round after the diagnosis | The user decides |
| None open | It merges by the project's merge rules (`fleet.md`) |

## Worked example

Synthetic. Pantry is a made-up recipe app. Its ticket #41, "Export a cookbook as one zip file", sits under spec #38, and its pull request is #57. First the full review:

```md
VERDICT: CHANGES at 4b1f0c9e2d7a5b3c8f6e1d0a9b2c4e7f5a3d1c08

Full review by Codex, Sol 6.1, Medium. CI green on this commit: build, test.

Intent: `pantry export` writes the whole cookbook to one zip file, and refuses to overwrite a file that is already there.

- **R1 [high]** `src/export.rs:48` (Spec) An existing `cookbook.zip` is overwritten without a word, so the user loses their last export; the ticket says export refuses. Fix: open the file with `create_new(true)`, and exit with status 3 when it exists.
  Shows the break: `touch cookbook.zip && pantry export; echo $?` printed `0`; the ticket needs `3`, with the file unchanged. Rerun at this commit: same.
- **R2 [medium]** `tests/export.rs:12` (Standards) The test now skips `.webp` photos, which relaxes the "every file is exported" check; the ticket didn't ask for it. Fix: drop the skip.
- **R3 [medium]** `src/export.rs:71` (Sweep) A recipe titled `Soup/Stew` becomes the zip entry `Soup/Stew.md`, a folder the importer rejects. Fix: replace `/` in entry names.
- **R4 [low]** `src/cli.rs:30` (Sweep) The help says "zip archive file", where the glossary's word is "export". Fix: "Write the cookbook as one export (a zip file)." Follow-up: #58.

Checked: Standards (`AGENTS.md`, `docs/agents/`, ADR 0002), Spec (#41, spec #38), full sweep. Ran: `cargo test` (31 passed); R1's command; `tests/export.rs` with the change undone (fails, as it should).

Next: the builder fixes R1, R2 and R3, or disputes any of them once. R4 doesn't block.
```

The builder fixed R1 and R2, and disputed R3 with evidence. The re-check:

```md
VERDICT: APPROVE at 9e8d7c6b5a4f3e2d1c0b9a8f7e6d5c4b3a2f1e0d

Re-check 1 of R1, R2, R3 and the code changed since 4b1f0c9, by Codex, Sol 6.1, Medium. CI green on this commit: build, test.

Intent: `pantry export` writes the whole cookbook to one zip file, and refuses to overwrite a file that is already there.

- **R1 [high]** resolved at 9e8d7c6: R1's command now prints `3`, and `cookbook.zip` is unchanged.
- **R2 [medium]** resolved at 9e8d7c6: the skip is gone, and `cargo test` exports the `.webp` photo.
- **R3 [medium]** withdrawn: the builder showed that titles are made safe before export (`src/recipe.rs:20`, test `slug_strips_slash`), so no `/` reaches the zip.
- **R4 [low]** follow-up #58.
- **R5 [low]** `src/export.rs:52` (Sweep) The new error says "file exist". Fix: "file exists". Follow-up: #59.

Checked: R1 to R3, and `git diff 4b1f0c9 9e8d7c6` with the three checks. Ran: R1's command (prints `3`); `cargo test` (33 passed).

Next: none open. It merges by the project's merge rules.
```

## Credit

This skill adapts three MIT-licensed skills:

- Matt Pocock's `code-review` (`github.com/mattpocock/skills` @ `a496f55`): the two checks, Standards and Spec, run and reported apart.
- Theo Browne's `sol-loop` (`github.com/t3dotgg/tstack` @ `1085050`): the verdict line, stable ids, severity by impact, re-checks of the open ids and changed code only, no model swapped in, and silence never counting as approval.
- Lauren Tan's `interrogate` (`github.com/cursor/plugins`, its `pstack` folder, @ `4dd1d05`): a finding proven by a run and rerun before it counts, and the intent restated first.

Matt Pocock's notice:

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

Theo Browne's notice:

Copyright (c) 2026 Theo Browne

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

Lauren Tan's notice:

Copyright (c) 2026 Lauren Tan

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
