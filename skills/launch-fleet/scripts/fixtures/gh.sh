#!/bin/sh
# A fake `gh` for fleet.sh's tests (test.sh puts it first on PATH as `gh`).
# It answers only the reads fleet.sh makes, from a synthetic GitHub in
# $FAKE_GH_WORLD (JSON, see check.json), and logs each call to $FAKE_GH_LOG.
# It also answers plan-graph's revision.sh (`issue view <n> --json ... --jq`)
# and `repo clone` (from the git repository $FAKE_GH_REMOTE).
# Any other call, including every write, fails, so the tests catch it.
#
# Faults, for the error cases:
#   FAKE_GH_FAIL=<text>    a call whose log line contains <text> fails (exit 1);
#                          an underscore in <text> stands for a space
#   FAKE_GH_ERRORS=<op>    GraphQL query <op> answers with `errors`, exit 0
#   FAKE_GH_PAGE=<n>       page size for the paginated reads (default 100)
#   FAKE_GH_SEARCH_CAP=<n> the search lists at most <n> issues, though it counts them all

world=${FAKE_GH_WORLD:?}
logfile=${FAKE_GH_LOG:?}
page=${FAKE_GH_PAGE:-100}

fail() { echo "fake gh: $*" >&2; exit 1; }
answer() { # LOGLINE: log it, then fail it if FAKE_GH_FAIL matches
  printf '%s\n' "$1" >> "$logfile" || fail "can't write the log"
  pattern=$(printf '%s' "${FAKE_GH_FAIL:-}" | tr _ ' ')
  case $pattern in ?*) case $1 in *"$pattern"*) fail "planted failure: $1" ;; esac ;; esac
}

repo=$(jq -er .repo "$world") || fail "no repo in the world"

if [ "$1 $2" = "repo view" ]; then
  [ "$*" = "repo view --json nameWithOwner --jq .nameWithOwner" ] || fail "unexpected: gh $*"
  answer "repo view"
  printf '%s\n' "$repo"
  exit 0
fi
if [ "$1 $2" = "repo clone" ]; then
  [ "$#" = 8 ] && [ "$3" = "$repo" ] && [ "$5 $6 $7 $8" = "-- -q --depth 1" ] || fail "unexpected: gh $*"
  answer "repo clone"
  exec git clone -q --depth 1 "file://${FAKE_GH_REMOTE:?}" "$4"
fi

if [ "$1 $2" = "issue view" ]; then
  [ "$#" = 7 ] && [ "$4" = --json ] && [ "$6" = --jq ] || fail "unexpected: gh $*"
  case $3 in '' | *[!0-9]*) fail "not an issue number: $3" ;; esac
  case $5 in subIssues | body | number,title,blockedBy) ;; *) fail "unexpected fields: $5" ;; esac
  answer "issue view $3 $5"
  jq -e --argjson n "$3" 'any(.issues[]; .number == $n)' "$world" > /dev/null || fail "Could not resolve to an issue: $3"
  one=$(mktemp) || fail "no temporary file"
  jq -r --argjson n "$3" --arg fields "$5" '. as $w | [.issues[] | select(.number == $n)][0]
    | {number, title, body: (.body // ""),
       subIssues: ([$w.issues[] | select(.parent == $n) | {number}] | {totalCount: length, nodes: .}),
       blockedBy: {nodes: [.blockedBy[]? | {number: .}]}}
    | with_entries(select(.key as $k | $fields | split(",") | index($k)))' "$world" > "$one" \
    || { rm -f "$one"; fail "the world is not valid"; }
  jq -r "$7" "$one"
  status=$?
  rm -f "$one"
  exit $status
fi

[ "$1" = api ] || fail "unexpected: gh $*"
shift

query='' owner='' name='' number='' cursor='' label='' state='' size='' accept='' paginate=no path=''
q='' ids='[]'
while [ $# -gt 0 ]; do
  case $1 in
    -f|-F)
      [ $# -ge 2 ] || fail "$1 needs a value"
      case $2 in
        query=*) query=${2#query=} ;;
        owner=*) owner=${2#owner=} ;;
        name=*) name=${2#name=} ;;
        number=*) number=${2#number=} ;;
        cursor=*) cursor=${2#cursor=} ;;
        label=*) label=${2#label=} ;;
        state=*) state=${2#state=} ;;
        size=*) size=${2#size=} ;;
        q=*) q=${2#q=} ;;
        'ids[]='*) ids=$(printf '%s' "$ids" | jq -c --arg i "${2#ids[]=}" '. + [$i]') || fail "bad id" ;;
        *) fail "unexpected field: $2" ;;
      esac
      shift 2 ;;
    -H)
      [ "${2:-}" = 'Accept: application/vnd.github.raw+json' ] || fail "unexpected header: ${2:-}"
      accept=raw; shift 2 ;;
    --paginate) paginate=yes; shift ;;
    -*) fail "not a read fleet.sh makes: $1" ;;
    *) [ -z "$path" ] || fail "two paths: $path $1"; path=$1; shift ;;
  esac
done

# Pages of a JSON array on stdin, as `gh api --paginate` prints them: one
# array per page, one after another.
pages() { jq -c --argjson n "$page" '. as $a | range(0; ([($a | length), 1] | max); $n) | $a[.:. + $n]'; }

if [ "$path" = graphql ]; then
  op=$(printf '%s' "$query" | sed -n 's/^ *query \([A-Za-z]*\)(.*/\1/p' | head -n 1)
  case $op in
    Closed | Nodes) [ -z "$owner$name" ] || fail "$op takes no owner or name" ;;
    *) [ "$owner/$name" = "$repo" ] || fail "graphql for $owner/$name, not $repo" ;;
  esac
  case $op in
    Closed)
      answer "graphql Closed cursor=${cursor:-none}"
      case $q in "repo:$repo is:issue is:closed label:\""*"\" no:parent-issue") ;; *) fail "unexpected search: $q" ;; esac
      label=${q#"repo:$repo is:issue is:closed label:\""}
      label=${label%"\" no:parent-issue"} ;;
    Nodes) answer "graphql Nodes count=$(printf '%s' "$ids" | jq length)" ;;
    Specs|Merged) answer "graphql $op cursor=${cursor:-none}" ;;
    Spec) answer "graphql Spec number=$number" ;;
    Singles) answer "graphql Singles state=$state size=$size label=$label cursor=${cursor:-none}" ;;
    *) fail "unknown query: $(printf '%s' "$query" | head -n 1)" ;;
  esac
  if [ "${FAKE_GH_ERRORS:-}" = "$op" ]; then
    echo '{"data":null,"errors":[{"message":"planted GraphQL error"}]}'
    exit 0
  fi
  # A query's own page size, when it sets one ($size), applies when it is smaller.
  case $size in '') ;; *[!0-9]*) fail "size is not a number: $size" ;; *) [ "$size" -lt "$page" ] && page=$size ;; esac
  jq -c --arg op "$op" --arg number "$number" --arg cursor "$cursor" --arg lbl "$label" --arg state "$state" \
    --argjson ids "$ids" --argjson cap "${FAKE_GH_SEARCH_CAP:-1000}" --argjson size "$page" '
    . as $w
    | def window($all):
        (if $cursor == "" then 0 else ($cursor | tonumber) end) as $from
        | {pageInfo: {hasNextPage: ($from + $size < ($all | length)), endCursor: (($from + $size) | tostring)},
           nodes: $all[$from:$from + $size]};
      def comments: {pageInfo: {hasPreviousPage: (.commentsTruncated // false)},
                      nodes: [.comments[]? | {databaseId: .id, createdAt: .at, body}]};
      def closing($n): [$w.pulls[] | select(any(.closes[]; . == $n))];
      def pr: {number, state, isDraft: (.draft // false), merged: (.state == "MERGED"), mergedAt,
               baseRefName: .base, headRefOid: .head,
               commits: {nodes: [{commit: {oid: .head, messageHeadline: .headline,
                 statusCheckRollup: (if .rollup == null then null else
                   {state: .rollup, contexts: {pageInfo: {hasNextPage: (.checksTruncated // false)},
                     nodes: [.checks[]? | {__typename: "CheckRun", name, status, conclusion}]}} end)}}]},
               comments: comments};
      def ticket: . as $t
        | {number, title, state, body: (.body // ""),
           labels: {pageInfo: {hasNextPage: false}, nodes: [.labels[]? | {name: .}]},
           timelineItems: {nodes: [.labelEvents[]? | {createdAt: .at, label: {name}}]},
           comments: comments,
           blockedBy: {pageInfo: {hasNextPage: (.blockersTruncated // false)},
             nodes: [$t.blockedBy[]? as $b | $w.issues[] | select(.number == $b)
               | {number, state, closedByPullRequestsReferences: {nodes: [closing(.number)[]
                   | {number, merged: (.state == "MERGED"), baseRefName: .base}]}}]},
           closedByPullRequestsReferences: {pageInfo: {hasNextPage: false}, nodes: [closing($t.number)[] | pr]}};
    if $op == "Closed" then
      [$w.issues[] | select(.state == "CLOSED" and .parent == null and any(.labels[]?; . == $lbl))]
      | sort_by(.updatedAt // "") | reverse | length as $count
      | {data: {search: ({issueCount: $count} + window(.[:$cap] | map({id: "I_\(.number)", number,
          comments: {nodes: [.comments[-1:][] | {body}]}})))}}
    elif $op == "Nodes" then
      {data: {nodes: [$ids[] as $id | $w.issues[] | select("I_\(.number)" == $id) | . as $i
        | ticket + {parent: (if .parent == null then null else {number: .parent} end),
                    subIssuesSummary: {total: ([$w.issues[] | select(.parent == $i.number)] | length)}}]}}
    elif $op == "Specs" then
      {data: {repository: {defaultBranchRef: {name: $w.default},
        issues: window([$w.issues[] | select(.state == "OPEN") | .number as $n
          | {number, subIssuesSummary: {total: ([$w.issues[] | select(.parent == $n)] | length)}, body: (.body // "")}])}}}
    elif $op == "Singles" then
      {data: {repository: {issues: window([$w.issues[] | select(.state == $state and any(.labels[]?; . == $lbl))]
        | sort_by(.updatedAt // "") | reverse
        | map(. as $i | ticket + {
            parent: (if .parent == null then null else {number: .parent} end),
            subIssuesSummary: {total: ([$w.issues[] | select(.parent == $i.number)] | length)}}))}}}
    elif $op == "Spec" then
      ([$w.issues[] | select(.number == ($number | tonumber))] | first) as $s
      | if $s == null then {data: {repository: {issue: null}}, errors: [{message: "Could not resolve to an Issue"}]}
        else {data: {repository: {issue: ($s | {number, title, state, comments: comments,
          subIssues: {pageInfo: {hasNextPage: (.subIssuesTruncated // false)},
            nodes: ([$w.issues[] | select(.parent == $s.number)] | sort_by(.number) | map(ticket))}})}}} end
    else
      {data: {repository: {pullRequests: window([$w.pulls[] | select(.state == "MERGED")]
        | sort_by(.updatedAt) | reverse
        | map({number, mergedAt, updatedAt, baseRefName: .base,
               closingIssuesReferences: {nodes: [.closes[] | {number: .}]}}))}}}
    end' "$world" || fail "the world is not valid"
  exit 0
fi

case $path in
  "repos/$repo/contents/"*)
    file=${path#"repos/$repo/contents/"}
    answer "contents $file"
    [ "$accept" = raw ] || fail "contents without the raw Accept header"
    jq -e --arg f "$file" '.files | has($f)' "$world" > /dev/null || fail "HTTP 404: $file"
    jq -j --arg f "$file" '.files[$f]' "$world" ;;
  "repos/$repo/pulls/"*"/files?per_page=100")
    n=${path#"repos/$repo/pulls/"}; n=${n%"/files?per_page=100"}
    answer "files $n"
    [ $paginate = yes ] || fail "files without --paginate"
    files=$(jq -ce --argjson n "$n" '[.pulls[] | select(.number == $n)] | first | .files' "$world") \
      || fail "HTTP 404: pull $n"
    printf '%s\n' "$files" | pages ;;
  "repos/$repo/git/matching-refs/"*)
    prefix=${path#"repos/$repo/git/matching-refs/"}
    answer "refs $prefix"
    jq -c --arg p "$prefix" '[.refs[] | select(("heads/" + .) | startswith($p))
      | {ref: ("refs/heads/" + .), object: {type: "commit", sha: ("0" * 40)}}]' "$world" ;;
  *) fail "unexpected read: $path" ;;
esac
