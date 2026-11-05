#!/bin/sh
# launch-fleet's helper (OpenWiz 0.0.5, the implementation). It reads a project's
# fleets from GitHub with `gh` and `jq`, and prints what is due. It never
# writes anything: the agent that reads its lines does the work.
#
#   sh fleet.sh [check]               every item due now, one line each; nothing when nothing is
#   sh fleet.sh frontier              every ticket that may start now: "#N title"
#                                     (a ticket of an approved plan graph, or an
#                                     issue approved on its own, its blockers merged)
#   sh fleet.sh holds                 every contract hold and builder note that is due
#   sh fleet.sh claim N COMMENT_ID    after a tick posts a claim on ticket or pull request N:
#                                     "go #N" if the claim is the oldest live one and N is
#                                     still due, else "withdraw #N: <why>"
#
# The repository is $GH_REPO (owner/name), else the clone you are in. It reads
# docs/agents/fleet.md (the CI checks) and docs/agents/triage-labels.md (the
# labels) from the default branch. The files of merged pull requests are cached
# in $LAUNCH_FLEET_CACHE (default ~/.cache/launch-fleet): they never change.
# Before a ticket of a spec may start, the spec's latest plan graph approval
# must be current: plan-graph's revision.sh ($LAUNCH_FLEET_REVISION, default
# ../../plan-graph/revision.sh from this file) prints the plan as it is now,
# and its SHA-256 must be the approval's fingerprint.
# $LAUNCH_FLEET_NOW (seconds since 1970) stands in for the clock in tests.
#
# Exit 0 with the answer on stdout. Exit 2 on any error, with the reason on
# stderr; never 1, which a CloudRoom loop's check takes as "nothing due".

die() { printf 'fleet.sh: %s\n' "$*" >&2; exit 2; }
usage() { die "usage: fleet.sh [check | frontier | holds | claim NUMBER COMMENT_ID]"; }
is_number() { case $1 in '' | *[!0-9]*) return 1 ;; esac; }

cmd=${1:-check}
[ $# -gt 0 ] && shift
case $cmd in
  check | frontier | holds) [ $# -eq 0 ] || usage ;;
  claim) [ $# -eq 2 ] && is_number "$1" && is_number "$2" || usage
    CLAIM_N=$1 CLAIM_ID=$2 ;;
  *) usage ;;
esac

command -v gh > /dev/null 2>&1 || die "gh is not installed"
command -v jq > /dev/null 2>&1 || die "jq is not installed"
if command -v sha256sum > /dev/null 2>&1; then SHA=sha256sum
elif command -v shasum > /dev/null 2>&1; then SHA='shasum -a 256'
else die "neither sha256sum nor shasum is installed"; fi
REVISION=${LAUNCH_FLEET_REVISION:-$(cd "$(dirname "$0")" && pwd)/../../plan-graph/revision.sh}

NOW=${LAUNCH_FLEET_NOW:-$(date -u +%s)}
is_number "$NOW" || die "LAUNCH_FLEET_NOW must be a number of seconds"

REPO=${GH_REPO:-}
if [ -z "$REPO" ]; then
  REPO=$(gh repo view --json nameWithOwner --jq .nameWithOwner) || die "set GH_REPO=owner/name, or run inside a clone"
fi
case $REPO in
  */*/* | /* | */ | *[!A-Za-z0-9._/-]*) die "GH_REPO must be owner/name, not '$REPO'" ;;
  ?*/?*) ;;
  *) die "GH_REPO must be owner/name, not '$REPO'" ;;
esac
OWNER=${REPO%%/*}
NAME=${REPO#*/}

TMP=$(mktemp -d) || die "can't make a temporary folder"
trap 'rm -rf "$TMP"' EXIT
trap 'exit 2' HUP INT TERM

# ---- GitHub reads ----

# The fields of a ticket, and of an issue approved on its own.
TICKET='number title state body
        labels(first: 20) { pageInfo { hasNextPage } nodes { name } }
        timelineItems(itemTypes: [LABELED_EVENT], last: 20) { nodes { ... on LabeledEvent { createdAt label { name } } } }
        comments(last: 50) { pageInfo { hasPreviousPage } nodes { databaseId createdAt body } }
        blockedBy(first: 20) {
          pageInfo { hasNextPage }
          nodes {
            number state
            closedByPullRequestsReferences(first: 5, includeClosedPrs: true) { nodes { number merged baseRefName } }
          }
        }
        closedByPullRequestsReferences(first: 5, includeClosedPrs: true) {
          pageInfo { hasNextPage }
          nodes {
            number state isDraft merged mergedAt baseRefName headRefOid
            commits(last: 1) {
              nodes {
                commit {
                  oid messageHeadline
                  statusCheckRollup {
                    state
                    contexts(first: 20) {
                      pageInfo { hasNextPage }
                      nodes {
                        __typename
                        ... on CheckRun { name status conclusion }
                        ... on StatusContext { context state }
                      }
                    }
                  }
                }
              }
            }
            comments(last: 50) { pageInfo { hasPreviousPage } nodes { databaseId createdAt body } }
          }
        }'

query() {
  case $1 in
    Specs) printf '%s\n' 'query Specs($owner: String!, $name: String!, $cursor: String) {
  repository(owner: $owner, name: $name) {
    defaultBranchRef { name }
    issues(states: OPEN, first: 100, after: $cursor) {
      pageInfo { hasNextPage endCursor }
      nodes { number subIssuesSummary { total } body }
    }
  }
}' ;;
    Spec) printf '%s\n' 'query Spec($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) {
    issue(number: $number) {
      number title state
      comments(last: 50) { pageInfo { hasPreviousPage } nodes { databaseId createdAt body } }
      subIssues(first: 100) {
        pageInfo { hasNextPage }
        nodes {' "$TICKET" '}
      }
    }
  }
}' ;;
    Singles) printf '%s\n' 'query Singles($owner: String!, $name: String!, $label: String!, $state: IssueState!, $size: Int!, $cursor: String) {
  repository(owner: $owner, name: $name) {
    issues(states: [$state], labels: [$label], first: $size, after: $cursor, orderBy: {field: UPDATED_AT, direction: DESC}) {
      pageInfo { hasNextPage endCursor }
      nodes {
        parent { number }
        subIssuesSummary { total }' "$TICKET" '}
    }
  }
}' ;;
    Closed) printf '%s\n' 'query Closed($q: String!, $cursor: String) {
  search(query: $q, type: ISSUE, first: 100, after: $cursor) {
    issueCount
    pageInfo { hasNextPage endCursor }
    nodes { ... on Issue { id number comments(last: 1) { nodes { body } } } }
  }
}' ;;
    Nodes) printf '%s\n' 'query Nodes($ids: [ID!]!) {
  nodes(ids: $ids) {
    ... on Issue {
      parent { number }
      subIssuesSummary { total }' "$TICKET" '}
  }
}' ;;
    Merged) printf '%s\n' 'query Merged($owner: String!, $name: String!, $cursor: String) {
  repository(owner: $owner, name: $name) {
    pullRequests(states: MERGED, first: 50, after: $cursor, orderBy: {field: UPDATED_AT, direction: DESC}) {
      pageInfo { hasNextPage endCursor }
      nodes { number mergedAt updatedAt baseRefName closingIssuesReferences(first: 10) { nodes { number } } }
    }
  }
}' ;;
  esac
}

# gql OP OUT [gh api arguments...]: one GraphQL read, checked.
gql() {
  op=$1 out=$2
  shift 2
  case $op in
    Closed | Nodes) ;;
    *) set -- -F owner="$OWNER" -F name="$NAME" "$@" ;;
  esac
  gh api graphql -f query="$(query "$op")" "$@" > "$out" \
    || die "the GitHub query $op failed"
  jq -e '(.errors // []) == [] and (.data | type) == "object" and all(.data[]; type == "object" or type == "array")' "$out" > /dev/null 2>&1 \
    || die "the GitHub query $op answered with errors: $(jq -c '.errors' "$out" 2> /dev/null | head -c 300)"
}

# raw PATH OUT: a file from the default branch.
raw() {
  gh api -H 'Accept: application/vnd.github.raw+json' "repos/$REPO/contents/$1" > "$2" \
    || die "can't read $1 from the default branch: run the setup skill first"
}

settings() {
  raw docs/agents/fleet.md "$TMP/fleet.md"
  raw docs/agents/triage-labels.md "$TMP/labels.md"
  awk '/^## /{on = ($0 == "## CI checks")} on && /^[|] *`[^`]+` *[|]/' "$TMP/fleet.md" \
    | sed 's/^| *`\([^`]*\)`.*/\1/' > "$TMP/checks.txt" || die "can't read the CI checks"
  [ -s "$TMP/checks.txt" ] || die "docs/agents/fleet.md lists no CI checks (its \"## CI checks\" table)"
  jq -Rn '[inputs]' < "$TMP/checks.txt" > "$TMP/checks.json" || die "can't read the CI checks"
  awk '/^## /{on = ($0 == "## Labels")} on && /^[|] *`[^`]+` *[|] *`[^`]+` *[|]/' "$TMP/labels.md" \
    | sed 's/^| *`\([^`]*\)` *| *`\([^`]*\)`.*/\1`\2/' > "$TMP/labels.txt" || die "can't read the labels"
  jq -Rn '[inputs | split("`") | {key: .[0], value: .[1]}] | from_entries' < "$TMP/labels.txt" > "$TMP/labels.json" \
    || die "can't read the labels"
  jq -e '. as $l | ["ready-for-agent", "needs-info", "needs-triage", "ready-for-human", "wontfix"]
    | all(.[]; . as $r | $l[$r] | type == "string")' "$TMP/labels.json" > /dev/null \
    || die "docs/agents/triage-labels.md does not map all five roles (its \"## Labels\" table)"
}

# specs: every open issue with sub-issues (a spec), read in full, into
# specs.json; every open follow-up filed outside its spec (its body's first
# line "Follow-up of #<ticket> (spec #<spec>)"), into followups.json; and the
# default branch into $DEFAULT.
specs() {
  : > "$TMP/spec-list.txt"
  : > "$TMP/followups.txt"
  cursor='' pages=0
  while :; do
    pages=$((pages + 1))
    [ $pages -le 50 ] || die "more than 5000 open issues: too many to read in one check"
    if [ -z "$cursor" ]; then gql Specs "$TMP/page.json"; else gql Specs "$TMP/page.json" -f cursor="$cursor"; fi
    DEFAULT=$(jq -er '.data.repository.defaultBranchRef.name' "$TMP/page.json") || die "no default branch"
    jq -r '.data.repository.issues.nodes[] | select(.subIssuesSummary.total > 0) | .number' "$TMP/page.json" \
      >> "$TMP/spec-list.txt" || die "can't read the open issues"
    jq -c '.data.repository.issues.nodes[]
      | ((.body // "") | gsub("\r"; "") | split("\n") | map(select(test("\\S"))) | (first // "") | sub("^\\s+"; "")) as $l
      | ([$l | capture("^Follow-up of #[0-9]+ \\(spec #(?<s>[0-9]+)\\)") | .s | tonumber] | first) as $spec
      | select($spec != null) | {number, spec: $spec}' "$TMP/page.json" >> "$TMP/followups.txt" || die "can't read the open issues"
    more=$(jq -r '.data.repository.issues.pageInfo.hasNextPage' "$TMP/page.json") || die "can't read the open issues"
    [ "$more" = true ] || break
    cursor=$(jq -er '.data.repository.issues.pageInfo.endCursor' "$TMP/page.json") || die "can't read the open issues"
  done
  printf '[' > "$TMP/specs.json" || die "can't write"
  sep=''
  for s in $(sort -n -u "$TMP/spec-list.txt"); do
    gql Spec "$TMP/spec-$s.json" -F number="$s"
    { printf '%s' "$sep" && jq -c '.data.repository.issue' "$TMP/spec-$s.json"; } >> "$TMP/specs.json" \
      || die "can't read spec #$s"
    sep=','
  done
  printf ']' >> "$TMP/specs.json" || die "can't write"
  jq -e 'type == "array" and all(.[]; type == "object")' "$TMP/specs.json" > /dev/null || die "a spec could not be read"
  jq -s '.' "$TMP/followups.txt" > "$TMP/followups.json" || die "can't read the follow-ups"
}

# singles: the issues labelled ready-for-agent outside any spec, which only
# an approval on their own can start, into singles.json: every open one, and
# every closed one whose agents may still need archiving. GitHub's search lists
# the closed ones (it leaves sub-issues out); each is read in full unless its
# last comment is its "archived" record. A search that lists fewer than it
# counts stops the check, so no archive is ever dropped.
singles() {
  label=$(jq -er '."ready-for-agent"' "$TMP/labels.json") || die "can't read the labels"
  : > "$TMP/singles.txt"
  cursor='' pages=0
  while :; do
    pages=$((pages + 1))
    [ $pages -le 40 ] || die "more than 2000 open issues labelled $label: too many to read in one check"
    if [ -z "$cursor" ]; then gql Singles "$TMP/page.json" -f label="$label" -f state=OPEN -F size=50
    else gql Singles "$TMP/page.json" -f label="$label" -f state=OPEN -F size=50 -f cursor="$cursor"; fi
    jq -c '.data.repository.issues.nodes[]' "$TMP/page.json" >> "$TMP/singles.txt" || die "can't read the issues"
    more=$(jq -r '.data.repository.issues.pageInfo.hasNextPage' "$TMP/page.json") || die "can't read the issues"
    [ "$more" = true ] || break
    cursor=$(jq -er '.data.repository.issues.pageInfo.endCursor' "$TMP/page.json") || die "can't read the issues"
  done
  q="repo:$REPO is:issue is:closed label:\"$label\" no:parent-issue"
  : > "$TMP/closed-ids.txt"
  cursor='' pages=0 listed=0 total=0
  while :; do
    pages=$((pages + 1))
    [ $pages -le 11 ] || die "the search for closed issues outside specs did not end"
    if [ -z "$cursor" ]; then gql Closed "$TMP/page.json" -f q="$q"; else gql Closed "$TMP/page.json" -f q="$q" -f cursor="$cursor"; fi
    total=$(jq -e '.data.search.issueCount | numbers' "$TMP/page.json") || die "can't read the closed issues"
    n=$(jq -e '.data.search.nodes | length' "$TMP/page.json") || die "can't read the closed issues"
    listed=$((listed + n))
    jq -r '.data.search.nodes[]
      | select((.comments.nodes[0].body // "") | gsub("\r"; "") | split("\n") | map(select(test("\\S"))) | (first // "")
               | test("^\\s*launch-fleet:\\s*archived\\b") | not)
      | .id' "$TMP/page.json" >> "$TMP/closed-ids.txt" || die "can't read the closed issues"
    more=$(jq -r '.data.search.pageInfo.hasNextPage' "$TMP/page.json") || die "can't read the closed issues"
    [ "$more" = true ] || break
    cursor=$(jq -er '.data.search.pageInfo.endCursor' "$TMP/page.json") || die "can't read the closed issues"
  done
  [ "$listed" = "$total" ] || die "the search listed $listed of $total closed issues outside specs, so an archive could be missed"
  set --
  while read -r id; do
    set -- "$@" -f "ids[]=$id"
    [ $# -lt 100 ] || { closed_nodes "$@"; set --; }
  done < "$TMP/closed-ids.txt"
  [ $# -eq 0 ] || closed_nodes "$@"
  jq -s '[.[] | select(.parent == null and .subIssuesSummary.total == 0)]' "$TMP/singles.txt" > "$TMP/singles.json" \
    || die "can't read the issues"
}

# closed_nodes -f ids[]=ID...: closed issues read in full, by id, into singles.txt.
closed_nodes() {
  gql Nodes "$TMP/nodes.json" "$@"
  jq -e --argjson n $(($# / 2)) '.data.nodes | length == $n and all(.[]; type == "object" and has("number"))' "$TMP/nodes.json" > /dev/null \
    || die "can't read every closed issue the search listed"
  jq -c '.data.nodes[]' "$TMP/nodes.json" >> "$TMP/singles.txt" || die "can't read the closed issues"
}

# The shared rules, in jq. $W is the world: {now, default, checks, labels,
# specs, singles, followups, claim, fingerprints, branches, merged}. $s is a ticket's spec, or null for
# an issue approved on its own.
LIB='
def lines: gsub("\r"; "") | split("\n");
def first_line: ([lines[] | select(test("\\S"))] | first) // "";
def rec: (first_line | sub("^\\s+"; "")) as $l
  | if ($l | startswith("launch-fleet:")) then ($l | ltrimstr("launch-fleet:") | sub("^\\s+"; "") | sub("\\s+$"; "")) else null end;
# In a claim ($W.claim: {n, id}), the claimed issue'"'"'s records are those older
# than the claim: the state the claim was made against.
def records: .number as $n | [.comments.nodes[] | {id: .databaseId, at: .createdAt, r: (.body | rec)} | select(.r != null)
  | select($W.claim == null or $n != $W.claim.n or .id < $W.claim.id)];
def plain: [.comments.nodes[] | select((.body | rec) == null)];
def kind($k): select(.r | test("^(" + $k + ")(\\b|$)"));
def age: $W.now - (.at | fromdateiso8601);
def thread: (.r | capture("thread (?<t>[^ ,;]+)") | .t) // null;
# A reviewer that went quiet, as its claim was edited: "review failed <commit>,
# thread <id>: <why>". That thread is never re-checked or told again.
def failed_thread: (.r | capture("^review failed [0-9a-f]{40}, thread (?<t>[^ ,;:]+)") | .t) // null;
def commits: [scan("\\b[0-9a-f]{40}\\b")];
def names($c): commits | any(.[]; . == $c);
def among($xs): . as $x | any($xs[]; . == $x);
# A "look" line for #$n, unless one was raised already (its record "look: <why>").
def look($n; $rs; $why): if any($rs[]; .r == "look: \($why)") then empty else "look #\($n): \($why)" end;

# A spec'"'"'s approved plan graph: its latest comment that starts
# "**Plan graph approved by" (the plan-graph skill'"'"'s record): the revision
# number, a table with one row per ticket (| #<n> | <title> | <blocked by> |
# <label> |), and "Fingerprint: <the SHA-256 of the revision text>".
def approval: (.body | lines) as $ls | ($ls | map(select(test("\\S"))) | first // "" | sub("^\\s+"; "")) as $head
  | if ($head | startswith("**Plan graph approved by")) | not then empty else
      {id: .databaseId, at: .createdAt,
       revision: ([$head | capture(", revision (?<n>[0-9]+):\\*\\*") | .n | tonumber] | first),
       fingerprint: ([$ls[] | capture("^\\s*Fingerprint:\\s*`?(?<f>[0-9a-f]{64})`?\\s*$") | .f] | first),
       rows: [$ls[] | select(test("^\\|\\s*#[0-9]+\\s*\\|")) | split("|") | map(sub("^\\s+"; "") | sub("\\s+$"; "")) | .[1:-1]
              | select(length >= 4) | {number: (.[0] | ltrimstr("#") | tonumber), label: .[-1]}]} end;
def latest($s): [$s.comments.nodes[] | approval] | max_by(.id);
def subs($s): [$s.subIssues.nodes[].number];
def row($s): . as $t | [latest($s).rows[]? | select(.number == $t.number)] | first;
# Current: the latest approval'"'"'s fingerprint is the one plan-graph'"'"'s
# revision.sh gives the plan now ($W.fingerprints, read only for a spec with a
# ticket that could start). Anything else is stale: nothing under it starts.
def current($s): latest($s).fingerprint as $f | $f != null and $f == $W.fingerprints["\($s.number)"];
def spec_rec($s; $re): any($s | records[]; .r | test($re));
# An issue outside any spec starts only on the user'"'"'s approval of it alone
# (the triage skill'"'"'s record): "**Approved on its own by <user>, <date>:**".
def own: [.comments.nodes[] | select(.body | lines | any(.[]; test("^\\s*\\*\\*Approved on its own by "))) | .createdAt] | max;
def is_approved($s): if $s == null then own != null else row($s).label == $W.labels["ready-for-agent"] end;
def approved_at($s): if $s == null then own else latest($s).at end;
# A person'"'"'s ticket: its row says ready-for-human. Never started, but held like any other.
def is_person($s): $s != null and row($s).label == $W.labels["ready-for-human"];
def is_planned($s): is_approved($s) or is_person($s);
def where($s): if $s == null then "approved on its own" else "spec #\($s.number)" end;

# A ticket.
def labels: [.labels.nodes[].name];
def has_role($role): $W.labels[$role] as $l | any(labels[]; . == $l);
# A ticket with the held label never starts.
def labelled_held: any(labels[]; . == "held");
def verified_merge: any(.closedByPullRequestsReferences.nodes[]?; .merged == true and .baseRefName == $W.default);
def merged_ok: .state == "CLOSED" and verified_merge;
def open_prs: [.closedByPullRequestsReferences.nodes[] | select(.state == "OPEN")];
def start: records as $rs
  | [$rs[] | kind("started|start checked")] as $live
  | [$rs[] | kind("starting")] as $claims
  | if ($live | length) > 1 then {state: "duplicate", rec: $live[0]}
    elif ($live | length) == 1 then {state: (if $live[0].r | test("^start checked") then "checked" else "started" end), rec: $live[0]}
    elif ($claims | length) > 0 then {state: "claimed", rec: ($claims | min_by(.id))}
    else {state: "queued", last: ([$rs[] | kind("start refused|start failed")] | max_by(.id))} end;
def builder: start as $st | if $st.rec then ($st.rec | thread) else null end;
def bthread: builder as $b | if $b then ", thread \($b)" else "" end;
def building: start.state != "queued" or (open_prs | length) > 0;
def looks: [
  (if .comments.pageInfo.hasPreviousPage then "more than 50 comments, so its records can'"'"'t all be read" else empty end),
  (if .labels.pageInfo.hasNextPage then "more than 20 labels" else empty end),
  (if .blockedBy.pageInfo.hasNextPage then "more than 20 blockers" else empty end),
  (if .closedByPullRequestsReferences.pageInfo.hasNextPage then "more than 5 linked pull requests" else empty end),
  (if start.state == "duplicate" then "more than one builder is recorded as started" else empty end),
  (open_prs as $o | if ($o | length) > 1 then "more than one open pull request: \($o | map("#\(.number)") | join(", "))" else empty end)];

# Contracts: the ticket'"'"'s "Relies on" line, and the merges that changed them.
def relies: ([(.body // "") | lines[] | select(test("^\\s*\\**Relies on:"))] | first // "") | [scan("`([^`]+)`") | .[0]];
def matches($p): . as $f
  | if ($p | endswith("/")) then ($f | startswith($p))
    elif ($p | contains("*")) then ($f | test("^" + ($p | gsub("(?<c>[.+?^${}()|\\[\\]\\\\])"; "\\\(.c)") | gsub("\\*"; "[^/]*")) + "$"))
    else $f == $p end;
def cutoff($s): approved_at($s);
def hits($s): . as $t | cutoff($s) as $c | relies as $ps
  | if $c == null or ($ps | length) == 0 then [] else
      [$W.merged[] | select(.closes as $cl | .mergedAt > $c and ($t.number | among($cl) | not))
       | {pr: .number, files: ([.files[] | select(.status != "added") | (.filename, .previous_filename)
           | select(. != null) | select(. as $f | any($ps[]; . as $p | $f | matches($p)))] | unique)}
       | select(.files | length > 0)] | sort_by(.pr) end;
def due_hits($s): records as $rs | [hits($s)[] | .pr as $p | select(any($rs[]; .r | test("^(held for|noted) #\($p)\\b")) | not)];
def held($s): ([records[] | kind("held for") | .at] | max) as $h | approved_at($s) as $a
  | $h != null and ($a == null or $h > $a);
# A hold of launch-fleet'"'"'s own that a newer approval has released. The held
# label on the ticket is launch-fleet'"'"'s only when launch-fleet'"'"'s "held for"
# record is no older than the label'"'"'s latest going on (its timeline): a held
# label put on later, by plan-graph or anyone, is theirs to take off.
def held_since: [.timelineItems.nodes[]? | select(.label.name == "held") | .createdAt] | max;
def released($s): held_since as $since | approved_at($s) as $a
  | ([records[] | kind("held for") | .at | select($since != null and . >= $since)] | max) as $h
  | $h != null and $a != null and $a > $h;

def ready_but_current($s):
  has_role("ready-for-agent")
  and ([has_role("needs-info"), has_role("needs-triage"), has_role("ready-for-human"), has_role("wontfix")] | any | not)
  and all(.blockedBy.nodes[]; merged_ok)
  and start.state == "queued"
  and (open_prs | length) == 0
  and (held($s) | not)
  and (labelled_held | not)
  and (due_hits($s) | length) == 0;
def ready($s): ready_but_current($s) and ($s == null or current($s));
# The specs whose approval must be checked: each with a ticket that could start.
def to_check: [$W.specs[] as $s | select(latest($s) != null)
  | select(any($s.subIssues.nodes[]; .state == "OPEN" and is_approved($s) and (looks | length) == 0 and ready_but_current($s)))
  | $s.number] | unique | .[];

# What each part of a spec has due.
def ticket_lines($s): . as $t
  | if .state == "CLOSED" then
      ([(records[] | kind("started|start checked") | thread),
        (.closedByPullRequestsReferences.nodes[] | records[] | (kind("review started") | thread), (kind("review failed") | failed_thread))]
       | map(select(. != null)) | unique) as $th
      | if ($th | length) > 0 and (any(records[]; .r | test("^archived\\b")) | not)
        then "archive #\(.number): closed; archive \($th | join(", "))" else empty end
    elif (is_planned($s) | not) then empty
    elif (looks | length) > 0 then records as $rs | looks[] | look($t.number; $rs; .)
    elif is_person($s) then
      (if labelled_held and released($s) then "release #\(.number): approved again since its hold; take the held label off" else empty end),
      (due_hits($s)[] | "hold #\($t.number): \(.files | join(", ")) changed in #\(.pr)"),
      (if has_role("ready-for-human") and (labelled_held | not) and (held($s) | not) and (due_hits($s) | length) == 0
          and all(.blockedBy.nodes[]; merged_ok) and (.blockedBy.pageInfo.hasNextPage | not)
          and (any(records[]; .r | test("^told the user\\b")) | not)
       then "person #\(.number): \(.title) (spec #\($s.number)) is ready, and only the user can do it" else empty end)
    else
      (if labelled_held and released($s) then "release #\(.number): approved again since its hold; take the held label off" else empty end),
      (building as $b | due_hits($s)[]
        | if $b then "note #\($t.number): \(.files | join(", ")) changed in #\(.pr); tell its builder\($t | bthread)"
          else "hold #\($t.number): \(.files | join(", ")) changed in #\(.pr)" end),
      (if has_role("needs-info") then
         ([plain[] | .createdAt] | max) as $q | ([records[] | kind("asked the orchestrator") | .at] | max) as $a
         | if $a == null or ($q != null and $q > $a) then "needs-info #\(.number): a question for the orchestrator" else empty end
       else empty end),
      (start as $st
       | if $st.state == "queued" then
           (if ready($s) then "start #\(.number): \(.title) (\(where($s))"
              + (if $st.last then "; retry after \"\($st.last.r)\"" else "" end) + ")" else empty end)
         elif $st.state == "claimed" then
           (if ($st.rec | age) >= 600 then "reconcile start #\(.number): claimed at \($st.rec.at), no outcome after 10 minutes" else empty end)
         elif $st.state == "started" then
           ($W.branches["\(.number)"] // []) as $br
           | if ($br | length) > 0 then "check start #\(.number): thread \($st.rec | thread), branch \($br | join(", "))"
             elif ($st.rec | age) >= 1800 then "check start #\(.number): thread \($st.rec | thread), no branch 30 minutes after the start"
             else empty end
         else empty end),
      (open_prs | map(select(.isDraft | not)) | first // empty | . as $p | .headRefOid as $head
       | records as $rs
       | "#\(.number) for #\($t.number) at \($head)" as $at
       | (.commits.nodes[0].commit // {}) as $c
       | if .baseRefName != $W.default then look(.number; $rs; "its base is \(.baseRefName), not \($W.default)")
         elif .comments.pageInfo.hasPreviousPage then look(.number; $rs; "more than 50 comments, so its records and reports can'"'"'t all be read")
         elif ($c.statusCheckRollup.contexts.pageInfo.hasNextPage // false) then look(.number; $rs; "more than 20 checks on its head")
         elif (($c.messageHeadline // "") | test("\\bgreen\\b"; "i") | not) then empty
         else
           ($c.statusCheckRollup) as $r
           | (if $r == null then "pending"
              elif $r.state == "FAILURE" or $r.state == "ERROR" then "failed"
              elif $r.state == "SUCCESS" and all($W.checks[]; . as $n | any($r.contexts.nodes[];
                     (.name // .context) == $n and (.conclusion // .state) == "SUCCESS")) then "green"
              else "pending" end) as $ci
           | ([$p.comments.nodes[] | select(.body | startswith("VERDICT: "))] | max_by(.databaseId)) as $v
           | ([$v.body // "" | first_line | capture("^VERDICT: (?<verdict>APPROVE|CHANGES) at (?<commit>[0-9a-f]{40})\\s*$")] | first) as $verdict
           | if $ci == "failed" then
               (if any($rs[]; (.r | test("^CI failure sent\\b")) and (.r | names($head))) then empty
                else "CI failed \($at): tell its builder\($t | bthread)" end)
             elif $ci != "green" then empty
             elif $v != null and $verdict == null then look($p.number; $rs; "its latest VERDICT line is not \"VERDICT: APPROVE|CHANGES at <the full commit>\"")
             elif $verdict != null and $verdict.commit == $head then
               (if $verdict.verdict == "APPROVE" then
                  (if any($rs[]; (.r | test("^merge blocked\\b")) and (.r | names($head))) then empty
                   else "merge \($at): APPROVE and CI green" end)
                else
                  (if any($rs[]; (.r | test("^findings sent\\b")) and (.r | names($head))) then empty
                   else "findings \($at): tell its builder\($t | bthread)" end) end)
             else
               [$rs[] | select(.r | names($head))] as $here
               | ([$here[] | kind("review started|re-check sent")] | max_by(.id)) as $live
               | if $live != null then
                   # A reviewer at work on this head, and no verdict on it yet: after 30
                   # minutes, tell it again once; 30 minutes after that, it has failed.
                   ($live | thread) as $th
                   | ([$here[] | kind("review told again") | select(thread == $th)] | max_by(.id)) as $told
                   | if $th == null then look($p.number; $rs; "\"\($live.r)\" names no thread, so its reviewer can'"'"'t be checked")
                     elif $told == null then
                       (if ($live | age) >= 1800 then "check review \($at): no verdict 30 minutes after \"\($live.r)\"; read thread \($th) and tell it again (its record: comment \($live.id))" else empty end)
                     elif ($told | age) >= 1800 then "check review \($at): still no verdict 30 minutes after it was told again; archive thread \($th) and record review failed (its record: comment \($live.id))"
                     else empty end
                 elif any($here[]; .r | test("^review starting\\b")) then
                   ([$here[] | select(.r | test("^review starting\\b"))] | min_by(.id)) as $cl
                   | if ($cl | age) >= 600 then "reconcile review \($at): claimed at \($cl.at), no outcome after 10 minutes" else empty end
                 else
                   [$rs[] | kind("review failed") | failed_thread | select(. != null)] as $quiet
                   | ([$rs[] | kind("review started") | select(thread | among($quiet) | not)] | max_by(.id)) as $first
                   | ([$here[] | kind("review refused|review failed")] | max_by(.id)) as $last
                   | (if $last == null then "" else " (retry after \"\($last.r)\")" end) as $retry
                   | if $first != null then "re-check \($at): tell its reviewer, thread \($first | thread)\($retry)"
                     elif ($quiet | length) > 0 then "review \($at): a fresh reviewer\($retry)"
                     else "review \($at): first review\($retry)" end
                 end
             end
         end)
    end;

def spec_lines: . as $s | records as $srs
  | if .subIssues.pageInfo.hasNextPage then look(.number; $srs; "more than 100 tickets, so its graph can'"'"'t all be read")
    elif .comments.pageInfo.hasPreviousPage then look(.number; $srs; "more than 50 comments, so its approvals can'"'"'t all be read")
    else
      latest($s) as $a
      | (if $a == null then empty
         elif ($a.rows | length) == 0 then look(.number; $srs; "its latest plan graph approval lists no tickets")
         else
           ($W.fingerprints["\($s.number)"] // null) as $now
           | if $now == null or current($s) then empty
             else "changed since revision \($a.revision // "?") (fingerprint now \($now[:12]))" as $what
             | if any($s | records[]; .r == "needs approval: \($what)") then empty
               else "re-approve #\($s.number): \($what)" end end
         end),
        ($s.subIssues.nodes[] | ticket_lines($s)),
        (if $a == null or ($a.rows | length) == 0 then empty
         elif all($s.subIssues.nodes[] | select(row($s) != null); .state == "CLOSED") then
           if spec_rec($s; "^spec summary\\b") | not then
             ([$s.subIssues.nodes[] | select(.state == "OPEN") | .number] + [$W.followups[] | select(.spec == $s.number) | .number]
              | unique) as $f
             | "spec done #\($s.number): every approved ticket is closed; "
               + (if ($f | length) == 0 then "no follow-ups" else "follow-ups to approve or drop: \($f | map("#\(.)") | join(" "))" end)
           elif all($s.subIssues.nodes[]; .state == "CLOSED")
                and (any($W.followups[]; .spec == $s.number) | not)
                and ([$s.subIssues.nodes[] | ticket_lines($s) | select(startswith("archive "))] | length) == 0
           then "close spec #\($s.number): every ticket is closed"
           else empty end
         else empty end)
    end;

def candidates: ($W.specs[] as $s | $s.subIssues.nodes[] | {t: ., s: $s}), ($W.singles[] | {t: ., s: null});
def check: [($W.specs | sort_by(.number)[] | spec_lines), ($W.singles | sort_by(.number)[] | ticket_lines(null))]
  | reduce .[] as $l ([]; . as $acc | if $l | among($acc) then . else . + [$l] end) | .[];
def frontier: [candidates | .s as $s | .t
  | select(.state == "OPEN" and is_approved($s) and (looks | length) == 0 and ready($s))
  | {number, title}] | unique_by(.number) | .[] | "#\(.number) \(.title)";
def holds: check | select(test("^(hold|note) "));
# The claim rule: a tick that posted a claim goes only if no older claim is
# still live (starting, started, start checked; or for a review, starting,
# started or re-check sent on the same commit), no builder or pull request is
# there, and the item is still due. A claim keeps its item as it is edited on.
def claim: $W.claim.n as $n | $W.claim.id as $id
  | ([candidates | select(.t.number == $n)] | first) as $c
  | if $c != null then
      ([$c.t.comments.nodes[] | select(.databaseId == $id)] | first) as $cm
      | if $cm == null then "error can'"'"'t find comment \($id) on #\($n)"
        elif ($cm.body | rec) != "starting" then "error comment \($id) on #\($n) is not a \"launch-fleet: starting\" claim"
        else $c.s as $s | $c.t | start as $st
          | if .state != "OPEN" then "withdraw #\($n): it is closed"
            elif (is_approved($s) | not) then "withdraw #\($n): it is not approved"
            elif $st.state != "queued" then "withdraw #\($n): an older claim or builder: \"\($st.rec.r)\""
            elif (open_prs | length) > 0 then "withdraw #\($n): pull request #\(open_prs[0].number) is open"
            elif (looks | length) > 0 then "withdraw #\($n): \(looks[0])"
            elif ready($s) then "go #\($n)"
            else "withdraw #\($n): it is not on the frontier now" end end
    else
      ([candidates | .s as $s | .t | select(.state == "OPEN" and is_approved($s)) | . as $t
        | .closedByPullRequestsReferences.nodes[] | select(.number == $n and .state == "OPEN") | {p: ., t: $t, s: $s}] | first) as $x
      | if $x == null then "error #\($n) is neither a ticket nor an open pull request of an approved ticket"
        else ([$x.p.comments.nodes[] | select(.databaseId == $id)] | first) as $cm
          | if $cm == null then "error can'"'"'t find comment \($id) on #\($n)"
            elif (($cm.body | rec // "") | test("^review starting [0-9a-f]{40}$") | not) then
              "error comment \($id) on #\($n) is not a \"launch-fleet: review starting <commit>\" claim"
            else ($cm.body | rec | capture("(?<c>[0-9a-f]{40})").c) as $sha
              | if $x.p.headRefOid != $sha then "withdraw #\($n): the head is now \($x.p.headRefOid)"
                else ([$x.p | records[] | select((.r | test("^(review starting|review started|re-check sent)\\b")) and (.r | names($sha)))]
                      | min_by(.id)) as $older
                  | if $older != null then "withdraw #\($n): an older review claim: \"\($older.r)\""
                    elif any($x.t | ticket_lines($x.s); startswith("review #\($n) for #\($x.t.number) at \($sha)")
                                                        or startswith("re-check #\($n) for #\($x.t.number) at \($sha)"))
                    then "go #\($n)"
                    else "withdraw #\($n): it is not due a review at \($sha)" end end end end end;
'

# jqw [-j] PROGRAM: run PROGRAM on the world, with the shared rules; -j
# prints raw text with no newline added.
jqw() {
  flag=-r
  if [ "$1" = -j ]; then flag=-j; shift; fi
  jq -n "$flag" --slurpfile w "$TMP/world.json" "\$w[0] as \$W | $LIB $1"
}

# world CHECKS LABELS SINGLES: what was read, as one JSON file.
world() {
  jq -n --argjson now "$NOW" --arg default "$DEFAULT" --slurpfile specs "$TMP/specs.json" \
    --slurpfile checks "$1" --slurpfile labels "$2" --slurpfile singles "$3" --arg claim "${CLAIM_N:+$CLAIM_N $CLAIM_ID}" \
    --slurpfile followups "$TMP/followups.json" \
    '{now: $now, default: $default, specs: $specs[0], singles: $singles[0], checks: $checks[0], labels: $labels[0],
      followups: $followups[0],
      claim: (if $claim == "" then null else ($claim | split(" ") | {n: (.[0] | tonumber), id: (.[1] | tonumber)}) end),
      fingerprints: {}, branches: {}, merged: []}' > "$TMP/world.json" || die "can't build the world"
}

# Each spec with a ticket that could start: its plan as it is now, from
# plan-graph's revision.sh, and that text's SHA-256. A read that fails stops
# the check: nothing starts on a guess.
fingerprints() {
  jqw 'to_check' > "$TMP/to-check.txt" || die "can't list the specs to check"
  printf '{' > "$TMP/fingerprints.json" || die "can't write"
  sep=''
  for s in $(sort -n -u "$TMP/to-check.txt"); do
    [ -f "$REVISION" ] || die "can't find plan-graph's revision.sh at $REVISION, so no approval can be checked"
    sh "$REVISION" "$s" > "$TMP/revision-$s.txt" || die "plan-graph's revision.sh failed on #$s, so its approval can't be checked"
    f=$($SHA < "$TMP/revision-$s.txt") || die "can't hash the revision of #$s"
    f=$(printf '%s' "$f" | cut -c1-64)
    case $f in *[!0-9a-f]* | '') die "can't hash the revision of #$s" ;; esac
    [ ${#f} = 64 ] || die "can't hash the revision of #$s"
    printf '%s"%s":"%s"' "$sep" "$s" "$f" >> "$TMP/fingerprints.json" || die "can't write"
    sep=','
  done
  printf '}' >> "$TMP/fingerprints.json" || die "can't write"
  jq --slurpfile f "$TMP/fingerprints.json" '.fingerprints = $f[0]' "$TMP/world.json" > "$TMP/w2.json" \
    && mv "$TMP/w2.json" "$TMP/world.json" || die "can't record the fingerprints"
}

# The branches of each started builder that is not checked yet.
branches() {
  jqw 'candidates | .s as $s | .t
    | select(.state == "OPEN" and is_approved($s) and (looks | length) == 0 and start.state == "started")
    | .number' > "$TMP/started.txt" || die "can't read the starts"
  printf '{' > "$TMP/branches.json"
  sep=''
  for n in $(sort -n -u "$TMP/started.txt"); do
    gh api "repos/$REPO/git/matching-refs/heads/room/$n-" > "$TMP/refs-$n.json" || die "can't read the branches of #$n"
    names=$(jq -ce 'if type == "array" then [.[].ref | ltrimstr("refs/heads/")] else error("not a list") end' "$TMP/refs-$n.json") \
      || die "can't read the branches of #$n"
    printf '%s"%s":%s' "$sep" "$n" "$names" >> "$TMP/branches.json" || die "can't write"
    sep=','
  done
  printf '}' >> "$TMP/branches.json"
  jq --slurpfile b "$TMP/branches.json" '.branches = $b[0]' "$TMP/world.json" > "$TMP/w2.json" && mv "$TMP/w2.json" "$TMP/world.json" \
    || die "can't record the branches"
}

# The merges since the oldest approval of a ticket that relies on a contract,
# with the files each one changed.
merges() {
  cutoff=$(jqw '[candidates | .s as $s | .t
      | select(.state == "OPEN" and is_planned($s) and (relies | length) > 0) | cutoff($s)] | map(select(. != null)) | min // ""') \
    || die "can't read the contracts"
  [ -n "$cutoff" ] || return 0
  : > "$TMP/merged.txt"
  cursor='' pages=0
  while :; do
    pages=$((pages + 1))
    [ $pages -le 20 ] || die "more than 1000 merges since $cutoff: too many to check for contract changes"
    if [ -z "$cursor" ]; then gql Merged "$TMP/page.json"; else gql Merged "$TMP/page.json" -f cursor="$cursor"; fi
    jq -c --arg c "$cutoff" --arg d "$DEFAULT" '.data.repository.pullRequests.nodes[]
      | select(.mergedAt != null and .mergedAt > $c and .baseRefName == $d)
      | {number, mergedAt, closes: [.closingIssuesReferences.nodes[].number]}' "$TMP/page.json" >> "$TMP/merged.txt" \
      || die "can't read the merges"
    more=$(jq -r --arg c "$cutoff" '.data.repository.pullRequests
      | if .pageInfo.hasNextPage and ([.nodes[].updatedAt] | min // "") >= $c then "yes" else "no" end' "$TMP/page.json") \
      || die "can't read the merges"
    [ "$more" = yes ] || break
    cursor=$(jq -er '.data.repository.pullRequests.pageInfo.endCursor' "$TMP/page.json") || die "can't read the merges"
  done
  cache=${LAUNCH_FLEET_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/launch-fleet}/$OWNER/$NAME
  mkdir -p "$cache" 2> /dev/null || cache=$TMP
  printf '[' > "$TMP/merged.json"
  sep=''
  while read -r line; do
    n=$(printf '%s' "$line" | jq -er .number) || die "can't read the merges"
    f=$cache/pull-$n-files.json
    if ! jq -e 'type == "array"' "$f" > /dev/null 2>&1; then
      gh api --paginate "repos/$REPO/pulls/$n/files?per_page=100" > "$TMP/files-$n.raw" || die "can't read the files of #$n"
      jq -s 'if all(.[]; type == "array") then add // [] | map({filename, status, previous_filename})
             else error("not a list") end' "$TMP/files-$n.raw" > "$TMP/files-$n.json" || die "can't read the files of #$n"
      cp "$TMP/files-$n.json" "$f.$$" 2> /dev/null && mv "$f.$$" "$f" 2> /dev/null || f=$TMP/files-$n.json
    fi
    printf '%s' "$sep" >> "$TMP/merged.json" && jq -c --slurpfile files "$f" '. + {files: $files[0]}' << EOF >> "$TMP/merged.json" || die "can't read the files of #$n"
$line
EOF
    sep=','
  done < "$TMP/merged.txt"
  printf ']' >> "$TMP/merged.json"
  jq --slurpfile m "$TMP/merged.json" '.merged = $m[0]' "$TMP/world.json" > "$TMP/w2.json" && mv "$TMP/w2.json" "$TMP/world.json" \
    || die "can't record the merges"
}

settings
specs
singles
world "$TMP/checks.json" "$TMP/labels.json" "$TMP/singles.json"
[ "$cmd" = check ] && branches
merges
[ "$cmd" = holds ] || fingerprints
jqw "$cmd" > "$TMP/out.txt" || die "can't work out what is due"
case $(head -n 1 "$TMP/out.txt") in error\ *) die "$(head -n 1 "$TMP/out.txt" | cut -c7-)" ;; esac
cat "$TMP/out.txt"
