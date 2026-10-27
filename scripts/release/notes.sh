#!/usr/bin/env bash
# Writes the release notes for a tag with Claude, from the PRs merged into main and the issues
# closed since the previous tag. Prints markdown on stdout; the PR list and the Full Changelog link
# are appended by GitHub (generate_release_notes), not by this script.
#
#   scripts/release/notes.sh [<prev-tag> <new-tag>]   # default: the two latest tags, or <new-tag> = HEAD
#
# Needs: git, gh (authenticated, read-only is enough), jq and claude (logged in, or ANTHROPIC_API_KEY).
# Env: REPO (default: from gh), BASE (default main), NOTES_TIMEOUT (default 5m), NOTES_MODEL,
# NOTES_DRY=1 (print the prompt and collected data instead of calling claude).
# The PR/issue text is untrusted, so claude runs with no tools: it only receives text and answers.
set -Eeuo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=${REPO:-$(gh repo view --json nameWithOwner --jq .nameWithOwner)}
BASE=${BASE:-main}
TIMEOUT=${NOTES_TIMEOUT:-5m}

new=${2:-HEAD}
if [ -n "${1:-}" ]; then
  prev=$1
else
  prev=$(git describe --tags --abbrev=0 "${new}^" 2>/dev/null) || { echo "notes: no previous tag" >&2; exit 1; }
fi
git rev-parse -q --verify "$prev^{commit}" >/dev/null || { echo "notes: unknown ref $prev" >&2; exit 1; }
git rev-parse -q --verify "$new^{commit}" >/dev/null || { echo "notes: unknown ref $new" >&2; exit 1; }

since=$(git log -1 --format=%cI "$prev")
shas=$(git rev-list "$prev..$new")
clip() { head -c 2000; }

# PRs merged into main whose merge commit is in the range, without the release PR
prs=$(gh pr list --repo "$REPO" --base "$BASE" --state merged --limit 200 --search "merged:>=${since%%T*}" \
  --json number,title,body,labels,mergeCommit,closingIssuesReferences |
  jq -c --arg shas "$shas" '($shas | split("\n")) as $in
    | .[] | select(.mergeCommit.oid as $o | $in | index($o)) | select(.title | test("^release v[0-9]") | not)')

gather() {
  echo "=== DATA ==="
  echo "Release: $new (previous: $prev). Repository: $REPO."
  echo
  echo "## Pull requests merged into $BASE"
  declare -A seen=()
  while IFS= read -r pr; do
    [ -n "$pr" ] || continue
    jq -r '"### PR #\(.number): \(.title)\nLabels: \([.labels[].name] | join(", "))\nCloses: \([.closingIssuesReferences[].number | "#\(.)"] | join(", "))"' <<<"$pr"
    jq -r '.body // ""' <<<"$pr" | clip
    printf '\n'
    for i in $(jq -r '.closingIssuesReferences[].number' <<<"$pr"); do seen[$i]=1; done
  done <<<"$prs"

  echo "## Issues closed in the period"
  closed=$(gh issue list --repo "$REPO" --state closed --limit 200 --search "closed:>=${since%%T*}" --json number,title,body)
  for i in "${!seen[@]}"; do
    jq -c --argjson n "$i" '.[] | select(.number == $n)' <<<"$closed" | grep -q . ||
      closed=$(jq -c --argjson e "$(gh issue view "$i" --repo "$REPO" --json number,title,body)" '. + [$e]' <<<"$closed")
  done
  jq -c '.[]' <<<"$closed" | while IFS= read -r is; do
    jq -r '"### Issue #\(.number): \(.title)"' <<<"$is"
    jq -r '.body // ""' <<<"$is" | clip
    printf '\n'
  done
}

# NOTES_DRY=1 prints what claude would receive, to check the collected context.
if [ -n "${NOTES_DRY:-}" ]; then
  cat "$SCRIPT_DIR/notes-prompt.md"; echo; gather
  exit 0
fi
{ cat "$SCRIPT_DIR/notes-prompt.md"; echo; gather; } |
  timeout "$TIMEOUT" claude -p --output-format text --tools "" ${NOTES_MODEL:+--model "$NOTES_MODEL"}
