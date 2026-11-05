"""A fake `gh` for find-work.sh's test: `gh api graphql` over a synthetic
repository, as GitHub and gh 2.100 answer it.

It honours what a broken fetch would get wrong:
  - pages: `issues(first: N)` serves N issues a page; N over 100 is GitHub's
    EXCESSIVE_PAGINATION error. Without --paginate, only the first page.
    --slurp wraps the pages in one array, and needs --paginate, as in gh.
  - `states: OPEN` (without it, closed issues come too) and
    `orderBy: {field: CREATED_AT, direction: ASC|DESC}` (default: ASC).
  - `comments(last: K)` or `comments(first: K)`, and `labels(first: K)`
    with its totalCount: only the comments and labels asked for.
  - `{owner}` and `{repo}` in -F values, as gh fills them in.
Anything else it does not know (a flag, a query shape) exits 3 on stderr.

The repository is a JSON file named by FAKE_GH_SCENARIO:
  {"issues": [{"number", "title", "state", "created", "milestone", "parent",
               "sub_issues", "labels": [...], "comments": [...]}],
   "fault": {"kind": "http" | "graphql" | "errors-exit-0" | "no-pages-exit-0"
                     | "open" | "close" | "short" | "repeat", "page": k}}
A fault hits page k (1-based): an HTTP error or a GraphQL error there; an
error page, or no pages at all, with exit 0 (no real gh does either); an
issue opened, or the
oldest closed, just after page k is served; page k served one issue short;
or page k serving the previous page's last issue in place of its first.
"""

import json
import os
import re
import sys

OWNER, REPO = "synthetic", "project"


def die(code, msg):
    sys.stderr.write(msg + "\n")
    sys.exit(code)


def parse_args(argv):
    if argv[:2] != ["api", "graphql"]:
        die(3, f"fake gh: unsupported command: {argv[:2]}")
    flags, fields, raw = set(), {}, {}
    rest = argv[2:]
    i = 0
    while i < len(rest):
        a = rest[i]
        if a in ("--paginate", "--slurp"):
            flags.add(a)
        elif a in ("-F", "--field", "-f", "--raw-field") and i + 1 < len(rest):
            key, sep, value = rest[i + 1].partition("=")
            if not sep:
                die(3, f"fake gh: {a} needs key=value")
            if a in ("-F", "--field"):
                value = value.replace("{owner}", OWNER).replace("{repo}", REPO)
                fields[key] = value
            else:
                raw[key] = value
            i += 1
        else:
            die(3, f"fake gh: unsupported argument: {a}")
        i += 1
    return flags, {**fields, **raw}


def arg(text, name):
    m = re.search(rf"\b{name}: *([A-Za-z0-9_$]+)", text)
    return m.group(1) if m else None


def main():
    flags, variables = parse_args(sys.argv[1:])
    if "--slurp" in flags and "--paginate" not in flags:
        die(1, "the `--slurp` option is only supported in conjunction with `--paginate`")
    query = variables.get("query", "")
    if variables.get("owner") != OWNER or variables.get("repo") != REPO:
        die(3, f"fake gh: owner and repo must be this repository's, not {variables.get('owner')}/{variables.get('repo')}")
    if "--paginate" in flags and not re.search(r"\$endCursor: *String", query):
        die(3, "fake gh: --paginate needs a query taking $endCursor: String")
    m = re.search(r"\bissues\(([^)]*)\)", query)
    if not m or "pageInfo" not in query or "totalCount" not in query:
        die(3, "fake gh: the query must read issues(...) with totalCount and pageInfo")
    issues_args = m.group(1)
    first = arg(issues_args, "first")
    if first is None or not first.isdigit():
        die(3, "fake gh: issues(...) needs first: N")
    first = int(first)
    if "--paginate" in flags and arg(issues_args, "after") != "$endCursor":
        die(3, "fake gh: --paginate needs issues(after: $endCursor)")
    open_only = arg(issues_args, "states") == "OPEN"
    order = re.search(r"orderBy: *\{ *field: *([A-Z_]+), *direction: *([A-Z]+) *\}", issues_args)
    if order and order.group(1) != "CREATED_AT":
        die(3, f"fake gh: unsupported orderBy field {order.group(1)}")
    descending = bool(order) and order.group(2) == "DESC"

    cm = re.search(r"\bcomments\((first|last): *([0-9]+)\)", query)
    lm = re.search(r"\blabels\(first: *([0-9]+)\)", query)
    if not cm or not lm:
        die(3, "fake gh: the query must read comments(first|last: K) and labels(first: K)")
    comment_end, comment_k, label_k = cm.group(1), int(cm.group(2)), int(lm.group(1))
    if first > 100 or comment_k > 100 or label_k > 100:
        n = max(first, comment_k, label_k)
        page = {"data": {"repository": None}, "errors": [{
            "type": "EXCESSIVE_PAGINATION",
            "message": f"Requesting {n} records on the connection exceeds the `first` limit of 100 records."}]}
        sys.stdout.write(json.dumps([page] if "--slurp" in flags else page))
        die(1, f"gh: Requesting {n} records on the connection exceeds the `first` limit of 100 records.")

    with open(os.environ["FAKE_GH_SCENARIO"]) as f:
        scenario = json.load(f)
    issues = scenario["issues"]
    fault = scenario.get("fault") or {}

    def listed():
        chosen = [i for i in issues if i["state"] == "OPEN" or not open_only]
        return sorted(chosen, key=lambda i: i["created"], reverse=descending)

    def node(i):
        comments = i["comments"][-comment_k:] if comment_end == "last" else i["comments"][:comment_k]
        out = {"number": i["number"], "title": i["title"]}
        if re.search(r"\bmilestone\b", query):
            out["milestone"] = {"number": 1} if i.get("milestone") else None
        if re.search(r"\bparent\b", query):
            out["parent"] = {"number": i["parent"]} if i.get("parent") else None
        if re.search(r"\bsubIssuesSummary\b", query):
            out["subIssuesSummary"] = {"total": i.get("sub_issues", 0)}
        out["labels"] = {"totalCount": len(i["labels"]), "nodes": [{"name": n} for n in i["labels"][:label_k]]}
        out["comments"] = {"nodes": [{"body": b} for b in comments]}
        return out

    pages, start, k = [], 0, 0
    while True:
        k += 1
        current = listed()
        if fault.get("page") == k and fault["kind"] == "no-pages-exit-0":
            sys.stdout.write("[]" if "--slurp" in flags else "")
            sys.exit(0)
        if fault.get("page") == k and fault["kind"] == "http":
            die(1, "gh: HTTP 502: Bad Gateway (https://api.github.com/graphql)")
        if fault.get("page") == k and fault["kind"] in ("graphql", "errors-exit-0"):
            pages.append({"data": None, "errors": [{"type": "INTERNAL", "message": "Something went wrong (synthetic)."}]})
            sys.stdout.write(json.dumps(pages if "--slurp" in flags else pages[-1]))
            if fault["kind"] == "graphql":
                die(1, "gh: Something went wrong (synthetic).")
            sys.exit(0)
        chunk = current[start:start + first]
        if fault.get("page") == k and fault["kind"] == "repeat" and start > 0:
            chunk = [current[start - 1]] + chunk[1:]
        if fault.get("page") == k and fault["kind"] == "short":
            chunk = chunk[:-1]
        end = start + first
        has_next = end < len(current)
        pages.append({"data": {"repository": {"issues": {
            "totalCount": len(current),
            "pageInfo": {"hasNextPage": has_next, "endCursor": f"cursor-{end}" if chunk else None},
            "nodes": [node(i) for i in chunk]}}}})
        if fault.get("page") == k and fault["kind"] == "open":
            top = max(i["number"] for i in issues) + 1
            issues.append({"number": top, "title": "Synthetic issue opened mid-read", "state": "OPEN",
                           "created": max(i["created"] for i in issues) + 1, "labels": [], "comments": []})
        if fault.get("page") == k and fault["kind"] == "close":
            current[0]["state"] = "CLOSED"
        start = end
        if "--paginate" not in flags or not has_next:
            break
    if "--slurp" in flags:
        sys.stdout.write(json.dumps(pages))
    else:
        sys.stdout.write("".join(json.dumps(p) for p in pages))


main()
