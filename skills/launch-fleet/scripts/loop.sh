#!/bin/sh
# The script of a project's launch-fleet loop (the implementation). A loop's script
# runs in the loop's own sandbox, which lasts between runs: this keeps a clone
# of the project there, brings it up to the default branch, and runs the check
# from it, so the check always has plan-graph's revision.sh beside it and runs
# the reviewed code on the default branch. Step 1 of the skill makes the
# loop's copy of this file with GH_REPO filled in.
# Exit as the check exits; 2 when the clone can't be made or updated.
GH_REPO=${GH_REPO:-}
die() { printf 'loop.sh: %s\n' "$*" >&2; exit 2; }
case $GH_REPO in ?*/?*) ;; *) die "GH_REPO must be owner/name" ;; esac
export GH_REPO
dir=${LAUNCH_FLEET_HOME:-$HOME/launch-fleet}/$GH_REPO
if [ -d "$dir/.git" ]; then
  git -C "$dir" fetch -q --depth 1 origin HEAD || die "can't fetch $GH_REPO"
  git -C "$dir" checkout -q --detach FETCH_HEAD || die "can't update the clone in $dir"
else
  mkdir -p "$(dirname "$dir")" || die "can't make $(dirname "$dir")"
  gh repo clone "$GH_REPO" "$dir" -- -q --depth 1 || die "can't clone $GH_REPO"
fi
exec sh "$dir/skills/launch-fleet/scripts/fleet.sh"
