#!/bin/sh
# Prints the revision text of a spec's plan graph, version 2, as the
# plan-graph skill defines it ("Is an approval current?" in SKILL.md): the
# spec's body, and each of its tickets' number, blockers, body and title. A
# body is hashed with its ticked task boxes read as unticked, so ticking one
# is never a change to the plan. Its SHA-256 is the fingerprint an approval
# records.
#
# Usage: sh revision.sh <spec issue number>
#   then: sha256sum on the output (on macOS: shasum -a 256)
# Runs only `gh issue view` reads. Exits 0 and prints the text; exits 1, and
# prints nothing on stdout, if any read fails; exits 2 on a bad argument.
set -u
spec=${1:-}
case $spec in
  '' | *[!0-9]*)
    echo "usage: sh revision.sh <spec issue number>" >&2
    exit 2
    ;;
esac
if command -v sha256sum > /dev/null 2>&1; then
  hash() { sha256sum < "$1" | cut -d' ' -f1; }
elif command -v shasum > /dev/null 2>&1; then
  hash() { shasum -a 256 < "$1" | cut -d' ' -f1; }
else
  echo "revision.sh: needs sha256sum or shasum" >&2
  exit 1
fi
fail() {
  echo "revision.sh: $*" >&2
  exit 1
}
work=$(mktemp -d) || fail "cannot make a temporary folder"
trap 'rm -rf "$work"' EXIT
tab=$(printf '\t')
cr=$(printf '\r')
# A body's hash: of the body with each ticked task box read as unticked. On a
# line that starts with spaces or tabs, a -, * or + and then spaces or tabs,
# an [x] or [X] followed by a space, a tab, a CR or the line's end becomes
# [ ]. Bytes, not characters (LC_ALL=C), so every machine reads the same.
unticked_hash() {
  LC_ALL=C sed -E -e "s/^([ $tab]*[-*+][ $tab]+)\[[xX]\]$/\1[ ]/" \
    -e "s/^([ $tab]*[-*+][ $tab]+)\[[xX]\]([ $tab$cr])/\1[ ]\2/" "$1" > "$1.unticked" || return 1
  hash "$1.unticked"
}

# The spec's tickets: its sub-issues, as many as GitHub counts.
gh issue view "$spec" --json subIssues --jq '.subIssues.totalCount, (.subIssues.nodes[].number)' > "$work/members" ||
  fail "cannot read spec #$spec's tickets"
total=$(head -n 1 "$work/members")
tail -n +2 "$work/members" | sort -n > "$work/tickets" || fail "cannot sort the tickets"
count=$(wc -l < "$work/tickets" | tr -d ' ')
[ "$count" = "$total" ] || fail "spec #$spec has $total tickets, but $count were listed"

gh issue view "$spec" --json body --jq '.body' > "$work/body" || fail "cannot read spec #$spec's body"
body_hash=$(unticked_hash "$work/body") || fail "cannot hash spec #$spec's body"
[ "${#body_hash}" = 64 ] || fail "cannot hash spec #$spec's body"
{
  echo "plan-graph revision v2"
  echo "spec #$spec body sha256 $body_hash"
} > "$work/out" || fail "cannot write the text"

while read -r n; do
  gh issue view "$n" --json number,title,blockedBy --jq '"#\(.number)\t\([.blockedBy.nodes[].number] | sort | map("#" + tostring) | join(" ") | if . == "" then "none" else . end)\t\(.title)"' > "$work/meta" ||
    fail "cannot read ticket #$n"
  gh issue view "$n" --json body --jq '.body' > "$work/body" || fail "cannot read ticket #$n's body"
  IFS=$tab read -r number blockers title < "$work/meta" || fail "cannot read ticket #$n's title and blockers"
  [ "$number" = "#$n" ] && [ -n "$blockers" ] && [ -n "$title" ] || fail "ticket #$n's title or blockers are missing"
  body_hash=$(unticked_hash "$work/body") || fail "cannot hash ticket #$n's body"
  [ "${#body_hash}" = 64 ] || fail "cannot hash ticket #$n's body"
  printf 'ticket %s blocked-by %s body sha256 %s title %s\n' "$number" "$blockers" "$body_hash" "$title" >> "$work/out" ||
    fail "cannot write the text"
done < "$work/tickets"

cat "$work/out"
