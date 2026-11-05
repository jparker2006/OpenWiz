---
name: grill
description: Grill the user on a design in rounds of three plain questions, each with a recommendation, and save every question and exact reply to docs/grills after every round. Use before a version, spec or skill is planned, when the user asks to grill, or to resume an open grill.
---

# Grill

Interview the user until a design is settled, at most three questions at a time, and write down every word as you go, so a fresh session (or `plan-graph`, which plans from it) can carry on without the chat. You find the facts; the user makes every decision. The grill is done only when the user confirms its summary.

## Where things go

| What | Where |
| --- | --- |
| The grill itself | `docs/grills/<date>-<topic>.md`, one file per grill, for example `docs/grills/2026-10-30-fleets-0.0.5.md`. `<date>` is the day it starts; `<topic>` is a few lowercase words joined by hyphens |
| The index | `docs/grills/README.md`: every grill, whether it is open or confirmed, and what it feeds |
| New terms | The glossary: `GLOSSARY.md` at the root, unless `docs/agents/domain.md` names another |
| Hard-to-reverse decisions | ADRs (one short file per decision) in `docs/adr/`, unless `docs/agents/domain.md` names another place |

Create each file the first time you have something to write in it. `plan-graph` reads all three: the grill, the glossary and the ADRs.

**Grills are private.** A grill keeps the user's exact words, which can include personal context.

- Keep it in the repository of the project it is about, and nowhere else.
- A public repository, issue or document may only summarise a grill, never copy it word for word, and only through the project's export review.
- If the project's repository is public, ask the user where its grills go before the first round.

## Start

1. **Read what is already settled:** the grill index, every grill on a related topic, the glossary and the ADRs. If the index lists an open grill on this topic, resume it: read its file and carry on after its last round.
2. **Map the design tree:** every decision the topic needs, and the decisions that hang off each one. The **frontier** is every open decision whose prerequisites are settled: what you can ask now without guessing at answers you haven't heard.
3. **Open the grill:** write its file from the template below and its row in the index (status **open**), on a branch of its own, with one pull request for this grill alone.

Done when the file, its index row and its pull request exist.

## Each round

1. **Check the grill log and ADRs first.** Re-read this grill, the other grills and the ADRs, and drop every question they already answer: a settled question is never re-asked. List what you dropped in the file's "Already settled" line, with where it was settled.
2. **Pick at most three questions from the frontier,** the ones that unblock the most. A question that hangs on another question still open belongs to a later round. Re-asked questions count toward the three. Facts are your job: anything the code, the files or the tracker can tell you, look up yourself, and ask the user only for decisions.
3. **Word each question in the simplest ELI5 way, without leaving out context.** Open with what it decides, in one short sentence. Then what is true today, the options, and what each one changes, in short everyday sentences; a concrete example beats a definition. Use the glossary's words, and explain any other term in a few words. Then give your recommendation on its own `➡️` line, worded so that "yes" accepts it.
4. **Save, then ask.** Add the round to the file and push. Then send the user the round's questions exactly as saved, so they can answer by id ("G2 yes").
5. **Wait for the reply.** Every decision is the user's, even one whose answer looks obvious.
6. **Save the reply exactly,** every line as they wrote it, typos and all, then the `Settled:` line (see "Reading a reply"), and push. Do this before anything else, so a session that ends here loses nothing.
7. **Record what the reply settled,** in this grill's pull request: each new term in the glossary, each hard-to-reverse decision as an ADR (below).
8. **Recompute the frontier** and start the next round.

Done when the reply, its `Settled:` line and any glossary or ADR change are pushed.

## Reading a reply

- **Settle a question only on a reply that answers it:** "yes", "rec is good", "G2 yes", or the user's own decision. A reply about something else, or one "yeah" that could mean anything, settles nothing: never infer an answer from a vague reply; ask again. A question the reply skips stays open too.
- **"Simplify this"** (or "I don't get it", or any reply asking what a question means): re-ask that question more simply in the next round, with the same id and the same context.
- **A question back:** answer it before the next round, and add your answer to the file as a note.
- **A reply that clashes with the glossary, an ADR, an earlier answer or the code:** say so plainly, and ask which one stands.
- **A fuzzy or overloaded word:** propose one precise term, and settle it like any other question.

## Finish

1. When the frontier is empty, **write a summary of at most 10 numbered lines,** one decision per line, in plain words.
2. **Save it, push, and show it to the user** exactly as saved. Ask them to confirm it.
3. **Save their reply exactly.** If it corrects a line, fix it, save the new summary below their reply, and show it again. If it opens a decision (something new, or something unclear), go back to rounds: ask it in the next round, with ids still counting, and when the frontier is empty again, write a new summary and show it.
4. **Done only when the user confirms the summary** (the latest one) and nothing is open. Set the file's status and its index row to **confirmed**, with the date, push, and merge the pull request as the project's merge rules allow (or ask the user to merge it).

Done when the confirmed grill is merged. From there, `plan-graph` can plan from it.

## The grill file

Every grill file has exactly this shape, from its first save. `<User>` is the user's name. An id is a letter or two for the topic and a number that keeps counting across rounds (G1, G2, G3, then G4 in round 2); a re-asked question keeps its id and says so.

```md
# Grill: <topic> (<YYYY-MM-DD>)

**Private.** Every question exactly as asked, with the recommendation; every reply exactly as <User> wrote it, typos included. Status: **open**. Feeds: <what it will feed>.

Already settled: <each settled decision this grill relies on, and where it was settled>.

## Round 1

**G1 - <the question, short>?** <its context, in plain words>
➡️ <the recommendation, worded so "yes" accepts it>

**G2 - <the question, short>?** <its context>
➡️ <the recommendation>

**G3 - <the question, short>?** <its context>
➡️ <the recommendation>

**<User>:**
> <their reply, exactly, every line>

Settled: G1, G3 (<a few words, if the reply changed the recommendation>). Open: G2.

## Round 2

**G2, re-asked - <the question, more simply>?** <its context>
➡️ <the recommendation>

...

## Summary (shown to <User> for confirmation)

1. <one decision per line, at most 10 lines>

**<User>:**
> <their reply, exactly>
```

- **Replies:** quote every message, in order, with `> ` before every line (a blank line inside a message is a lone `>`). Never tidy, shorten or fix a reply.
- **`Settled:` and `Open:`:** after each reply, every question of that round in one of the two (`none` when empty).
- **Your notes:** one line in parentheses under the reply, only when needed: the answer to a question back, why a question is re-asked, or the ADR a decision went to.
- **A corrected summary:** saved under `## Summary, corrected (shown to <User> for confirmation)`, below the reply that changed it, or below the rounds that reply opened.
- **Confirmed:** the status becomes `Status: **confirmed** by <User>, <YYYY-MM-DD>.` and drops "open".
- **"Already settled":** leave it out when nothing is.

The index, `docs/grills/README.md`:

```md
# Grills

**Private.** One file per grill, each saved exactly, question by question, after every round.

| Grill | Status | Feeds |
| --- | --- | --- |
| [<topic>](<YYYY-MM-DD>-<topic>.md) | **open** | <what it feeds> |
```

A confirmed row reads `**confirmed** (<YYYY-MM-DD>)`.

## Glossary and ADRs

**A new term:** add it to the glossary the moment it is settled, in the glossary's own format:

```md
**<Term>**:
<one or two sentences: what it is, not how it works>
_Avoid_: <other words for the same thing>
```

Only words specific to this project; general programming words stay out.

**An ADR:** write one for every settled decision that would be hard to reverse, even an obvious one. Name it `<NNNN>-<slug>.md`, numbered one above the highest in `docs/adr/`, and keep it to a title and one paragraph:

```md
# <the decision, as a short title>

<one to three sentences: the context, the decision with the user's own words quoted and dated (<User>, <YYYY-MM-DD>: "..."), and why>
```

## Licence

Adapted from Matt Pocock's `grilling` and `domain-modeling` skills (`github.com/mattpocock/skills` @ `a496f55`), which carry this notice:

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
