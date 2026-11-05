# Out of scope

The `.out-of-scope/` folder at the project's root records each request the user has rejected. It keeps the reason after the issue closes, and when the same request comes back, triage shows the earlier decision instead of arguing it again from the start.

## One file per idea

`.out-of-scope/<idea>.md`, named in a few lowercase words joined by hyphens, so someone browsing the folder can tell what was rejected without opening it (`dark-mode.md`, `plugin-system.md`). Every issue that asks for the same thing goes in the same file.

## The file

Write it as a short note for someone meeting the question for the first time: what was rejected, why, and every issue that asked for it.

```markdown
# Dark mode

This project does not support dark mode or user-chosen themes.

## Why this is out of scope

The pages are drawn from one fixed set of colours, chosen when the project
is built. Themes would need every page to choose its colours while it runs,
and somewhere to keep each user's choice. That is a large change for
something outside the project's purpose.

## Prior requests

- #42: "Add dark mode support"
- #87: "Night theme for accessibility"
```

The reason should still hold in a year: the project's purpose, a technical limit, or a decision and why it was made. "We're busy right now" is a postponement, not a rejection, so it doesn't belong here.

## Checking it

In step 1 of "Sort an issue", read every file in `.out-of-scope/`. Match by idea, not by keyword: "night theme" matches `dark-mode.md`. On a match, ask the user: "This looks like `.out-of-scope/dark-mode.md`, rejected because <reason>. Does that still hold?" Then:

| The user says | Then |
| --- | --- |
| It still holds | Add the issue to the file's "Prior requests", then close it as `wontfix` |
| They have changed their mind | Delete or update the file, and sort the issue as usual |
| It is a different request | Sort the issue as usual |

## Writing it

Only for an **enhancement** the user rejects, an outside pull request included. Never for one closed because it is already built: that is a built feature, not a rejected one, and a file here would wrongly match the next request for it. Its closing comment says where the feature lives instead.

1. Add the issue to the matching file's "Prior requests", or create the file.
2. Open a pull request with the change, merged by the project's standing merge rules (`docs/agents/fleet.md`).
3. Comment on the issue with the decision and a link to the file, then close it as `wontfix`.

If the user later changes their mind about an idea, delete its file. The old issues stay closed; the new request is sorted as usual.

Adapted from Matt Pocock's `OUT-OF-SCOPE.md` in his `triage` skill, under the MIT License; the notice is in [SKILL.md](SKILL.md#credit).
