# Issue tracker: GitHub

This project's versions, specs and tickets live in its GitHub repository. Use the `gh` CLI (2.94 or later, for sub-issues and "blocked by" links) for everything. Inside a clone, `gh` finds the repository from `git remote -v`.

## Where things live

| Thing | On GitHub |
| --- | --- |
| Version | A milestone, named after the version |
| Spec | An issue in that milestone; its body is the spec |
| Ticket | A sub-issue of its spec, with "blocked by" links to the tickets it waits for |
| A ticket's work | Its pull request |
| The user's approval of a spec or a plan graph | A comment on the spec issue, quoting the user and naming the revision approved |

A ticket is ready when it has the `ready-for-agent` label and every ticket blocking it has a verified merge: a closed blocker alone is not enough. A ticket closes only on a verified merge, a spec only when all its tickets are closed, and a version only when its milestone is done and its live check has passed.

## Commands

- **Create an issue:** `gh issue create --title "..." --body "..."`. Use a heredoc for a body of more than one line.
- **Put it in a version:** add `--milestone "<version>"`. List the versions with `gh api 'repos/{owner}/{repo}/milestones' --jq '.[] | {number, title, state}'`; create one with `gh api --method POST 'repos/{owner}/{repo}/milestones' -f title="<version>"`.
- **Make a ticket a sub-issue of its spec:** `gh issue create --parent <spec> ...`, or afterwards `gh issue edit <spec> --add-sub-issue <ticket>`.
- **Blocking:** GitHub's own "blocked by" links, which show on the issue page. Add them with `gh issue create --blocked-by <n>,<n> ...`, or afterwards `gh issue edit <ticket> --add-blocked-by <blocker>`.
- **List a ticket's blockers:** `gh issue view <n> --json blockedBy --jq '.blockedBy.nodes[] | {number, state}'`. A closed blocker still needs its verified merge before the ticket is ready.
- **Read an issue:** `gh issue view <n> --json number,title,body,labels,comments,milestone,parent,blockedBy`.
- **List issues:** `gh issue list --state open --json number,title,body,labels,comments --jq '[.[] | {number, title, body, labels: [.labels[].name], comments: [.comments[].body]}]'`, with `--label`, `--milestone` and `--state` filters as needed.
- **Comment:** `gh issue comment <n> --body "..."`.
- **Labels:** `gh issue edit <n> --add-label "..."` and `--remove-label "..."`.
- **Close:** post the reason with `gh issue comment <n> --body "..."`, then `gh issue close <n>`. On an issue that is already closed, `gh issue close --comment` drops the comment and still exits 0.

File issues only in this project's own repository. A fork can make `gh` default to its parent, so before the first `gh issue create` of a session, check the target with `gh repo view --json nameWithOwner`.

## Pull requests as triage input

**Outside pull requests count as requests: no.** _(Set to `yes` if this project treats pull requests from outside contributors as feature requests; the `triage` skill reads this.)_

When it is `yes`, those pull requests get the same labels and states as issues, with the `gh pr` commands: `gh pr view <n>`, `gh pr view <n> --comments`, `gh pr diff <n>`, `gh pr comment`, `gh pr edit --add-label` and `--remove-label`, and `gh pr close`. Issues and pull requests share one set of numbers, so for a bare `#42` try `gh pr view 42`, then `gh issue view 42`.

## When a skill says

- **"Publish to the issue tracker":** create a GitHub issue.
- **"Fetch the ticket":** read it as in **Read an issue** above.
