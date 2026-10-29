#!/usr/bin/env bash
# Adds a release to CHANGELOG.md from its GitHub Release description (read on stdin).
#
#   gh release view v0.9.1 --json body --jq .body | scripts/release/changelog.sh v0.9.1 [YYYY-MM-DD]
#
# The notes written by Claude (scripts/release/notes.sh) become the entry, with their `## ` headings
# turned into `### `. GitHub's generated part ("## What's Changed", "## New Contributors", the
# "Full Changelog" link) is dropped; when there are no Claude notes, its PR list becomes the entry.
# Adds the `[X.Y.Z]: compare` link too. Does nothing when the version is already in the file.
# Env: CHANGELOG (default CHANGELOG.md), REPO (default RafaelKC/twig).
set -Eeuo pipefail

tag=${1:?usage: changelog.sh vX.Y.Z [YYYY-MM-DD] < release-body.md}
date=${2:-$(date -u +%F)}
file=${CHANGELOG:-CHANGELOG.md}
repo=${REPO:-RafaelKC/twig}
version=${tag#v}

[ -f "$file" ] || { echo "changelog: $file not found" >&2; exit 1; }
if grep -qF "## [$version]" "$file"; then
  echo "changelog: $version is already in $file" >&2
  exit 0
fi

body=$(tr -d '\r')

# Claude's notes: everything before GitHub's generated part
notes=$(printf '%s\n' "$body" | awk '/^## (What'"'"'s Changed|New Contributors)/ || /^\*\*Full Changelog\*\*/ { exit } { print }')
if [ -z "$(printf '%s' "$notes" | tr -d '[:space:]')" ]; then
  # GitHub's list only: "* title by @user in https://github.com/o/r/pull/N" -> "- title (#N)"
  notes=$(printf '%s\n' "$body" | awk '
    /^## What'"'"'s Changed/ { on = 1; next }
    /^## / || /^\*\*Full Changelog\*\*/ { on = 0 }
    on && /^\* / {
      line = substr($0, 3)
      n = line; sub(/.*\/pull\//, "", n)
      sub(/ by @[^ ]+ in https:\/\/[^ ]+$/, "", line)
      if (line ~ /^release v[0-9]/) next
      if (!printed++) print "## Changes\n"
      print "- " line " (#" n ")"
    }')
fi
[ -n "$(printf '%s' "$notes" | tr -d '[:space:]')" ] || notes="No user-facing changes."

# heading levels one down, blank lines at the start and end trimmed
entry=$(printf '%s\n' "$notes" | sed -E 's/^(#{2,5}) /#\1 /' | awk 'NF { started = 1 } started' |
  awk '{ lines[NR] = $0 } NF { last = NR } END { for (i = 1; i <= last; i++) print lines[i] }')

prev=$(sed -n -E 's/^## \[([0-9]+\.[0-9]+\.[0-9]+)\].*/\1/p' "$file" | head -n 1)
if [ -n "$prev" ]; then
  link="[$version]: https://github.com/$repo/compare/v$prev...$tag"
else
  link="[$version]: https://github.com/$repo/releases/tag/$tag"
fi

tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT
ENTRY="## [$version] - $date"$'\n\n'"$entry" LINK=$link awk '
  # the new entry goes before the first release heading (or at the end of the intro)
  !entry_done && /^## \[/ { print ENVIRON["ENTRY"]; print ""; entry_done = 1 }
  # its link goes before the first release link
  !link_done && /^\[[0-9]+\.[0-9]+\.[0-9]+\]: / { print ENVIRON["LINK"]; link_done = 1 }
  { print }
  END {
    if (!entry_done) { print ""; print ENVIRON["ENTRY"] }
    if (!link_done) { print ""; print ENVIRON["LINK"] }
  }' "$file" >"$tmp"
cat "$tmp" >"$file"
echo "changelog: added $version to $file" >&2
