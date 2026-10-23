#!/usr/bin/env bash
# Autonomous issue -> PR loop, run by .github/workflows/agent.yml on a self-hosted runner.
#
#   implement (claude) -> push + PR -> review (claude, fresh session) -> fix -> ... -> PR gates -> done
#
# Labels on the issue (and the PR): agent:doing while working, agent:done or agent:fail at the end.
# Re-adding `agent` runs it again on the same branch/PR, with your new comments as input.
#
# Required env: GH_TOKEN (PAT), REPO (owner/name), ISSUE_NUMBER.
# Optional env: AGENT_OWNER, AGENT_HOME, AGENT_BASE, AGENT_MODEL, AGENT_MAX_REVIEW_ROUNDS,
#               AGENT_MAX_CI_ROUNDS, AGENT_CLAUDE_TIMEOUT, AGENT_CI_TIMEOUT, RUN_URL.
set -Eeuo pipefail

: "${GH_TOKEN:?GH_TOKEN (the AGENT_PAT secret) is required}"
: "${REPO:?}" "${ISSUE_NUMBER:?}"
OWNER=${AGENT_OWNER:-RafaelKC}
BASE=${AGENT_BASE:-main}
MAX_REVIEW_ROUNDS=${AGENT_MAX_REVIEW_ROUNDS:-3}
MAX_CI_ROUNDS=${AGENT_MAX_CI_ROUNDS:-3}
CLAUDE_TIMEOUT=${AGENT_CLAUDE_TIMEOUT:-45m}
CI_TIMEOUT=${AGENT_CI_TIMEOUT:-5400} # seconds to wait for the PR gates
RUN_URL=${RUN_URL:-}
HOME_DIR=${AGENT_HOME:-$HOME/agent}

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
N=$ISSUE_NUMBER
BRANCH=agent/issue-$N
REPO_DIR=$HOME_DIR/repo
WT=$HOME_DIR/work/issue-$N
LOG_DIR=$HOME_DIR/logs/issue-$N-$(date +%Y%m%d-%H%M%S)
MARK='<!-- twig-agent -->'
PR=

# The subscription (OAuth login) must be used, never a metered API key left in the environment.
unset ANTHROPIC_API_KEY

mkdir -p "$LOG_DIR" "$HOME_DIR/work"
export CARGO_TARGET_DIR=$HOME_DIR/target # shared build cache between runs

say() { gh issue comment "$N" --repo "$REPO" --body "$MARK"$'\n'"$1" >/dev/null; }
die() { echo "$1" >"$LOG_DIR/reason"; echo "agent: $1" >&2; exit 1; }

set_state() { # doing | done | fail: set agent:<state>, drop the other agent labels
  local s=$1 l rm=()
  for l in agent agent:doing agent:done agent:fail; do [ "$l" = "agent:$s" ] || rm+=("$l"); done
  local rmcsv; rmcsv=$(IFS=,; echo "${rm[*]}")
  gh issue edit "$N" --repo "$REPO" --add-label "agent:$s" >/dev/null
  gh issue edit "$N" --repo "$REPO" --remove-label "$rmcsv" >/dev/null 2>&1 || true
  if [ -n "$PR" ]; then
    gh pr edit "$PR" --repo "$REPO" --add-label "agent:$s" >/dev/null 2>&1 || true
    gh pr edit "$PR" --repo "$REPO" --remove-label "$rmcsv" >/dev/null 2>&1 || true
  fi
}

on_error() {
  local line=$1
  trap - ERR
  [ -e "$LOG_DIR/failed" ] && exit 1 # a subshell already reported it
  touch "$LOG_DIR/failed"
  local reason; reason=$(cat "$LOG_DIR/reason" 2>/dev/null || echo "unexpected error (run.sh line $line)")
  set_state fail || true
  say "❌ **agent:fail** — $reason"$'\n\n'"Run: ${RUN_URL:-n/a}"$'\n'"Re-add the \`agent\` label to try again." || true
  exit 1
}
trap 'on_error $LINENO' ERR
trap 'git -C "$REPO_DIR" worktree remove --force "$WT" >/dev/null 2>&1 || true' EXIT

# --- guards ------------------------------------------------------------------------------------

assoc=$(gh api "repos/$REPO/issues/$N" --jq .author_association)
case $assoc in
  OWNER | MEMBER | COLLABORATOR) ;;
  *) die "the issue was opened by an outside user ($assoc); its text is untrusted. Copy what you want done into a comment of your own and re-add the label." ;;
esac

for l in "agent:doing|fbca04" "agent:done|0e8a16" "agent:fail|b60205"; do
  gh label create "${l%|*}" --repo "$REPO" --color "${l#*|}" --force >/dev/null 2>&1 || true
done

# --- context -----------------------------------------------------------------------------------

find_pr() {
  gh pr list --repo "$REPO" --head "$BRANCH" --state open --json number --jq '.[0].number // empty'
}

# Only the maintainer's text is trusted; the agent's own comments (marked) are left out.
owner_comments() { # $1 = api path
  gh api --paginate "$1" | jq -r --arg o "$OWNER" --arg m "$MARK" '
    .[] | select(.user.login == $o and ((.body // "") | contains("<!-- twig-agent") | not))
    | "### " + .created_at + (if .path then " — " + .path + ":" + ((.line // .original_line // 0) | tostring) else "" end)
      + "\n" + .body + "\n"'
}

build_context() {
  local title body
  title=$(gh issue view "$N" --repo "$REPO" --json title --jq .title)
  body=$(gh issue view "$N" --repo "$REPO" --json body --jq .body)
  printf '# Issue #%s: %s\n\n%s\n\n## Comments from the maintainer (%s)\n\n' "$N" "$title" "$body" "$OWNER"
  owner_comments "repos/$REPO/issues/$N/comments"
  if [ -n "$PR" ]; then
    printf '\n## Maintainer feedback on the existing PR #%s\n\n' "$PR"
    owner_comments "repos/$REPO/issues/$PR/comments"
    owner_comments "repos/$REPO/pulls/$PR/comments"
    gh api --paginate "repos/$REPO/pulls/$PR/reviews" | jq -r --arg o "$OWNER" '
      .[] | select(.user.login == $o and ((.body // "") != "")) | "### review (" + .state + ")\n" + .body + "\n"'
  fi
}

# --- claude ------------------------------------------------------------------------------------

# claude_run <name> <allowed-tools> [extra claude args]; the prompt is on stdin.
claude_run() {
  local name=$1 tools=$2; shift 2
  (cd "$WT" && timeout "$CLAUDE_TIMEOUT" claude -p --output-format text --permission-mode acceptEdits \
    --allowedTools "$tools" "$@" ${AGENT_MODEL:+--model "$AGENT_MODEL"}) | tee "$LOG_DIR/$name.log"
}

BUILD_TOOLS='Bash,Edit,Write,Read,Grep,Glob'
REVIEW_TOOLS='Bash(git:*),Bash(cargo:*),Read,Grep,Glob'

# implement <name> <prompt-file> <extra-context-file>
implement() {
  cat "$SCRIPT_DIR/prompts/$2" "$3" | claude_run "$1" "$BUILD_TOOLS" >/dev/null
  if grep -q '^AGENT_RESULT: BLOCKED' "$LOG_DIR/$1.log"; then
    die "the agent could not proceed: $(grep '^AGENT_RESULT: BLOCKED' "$LOG_DIR/$1.log" | tail -n 1 | sed 's/^AGENT_RESULT: BLOCKED *//')"
  fi
  # whatever the agent forgot to commit still goes in
  if [ -n "$(git -C "$WT" status --porcelain)" ]; then
    git -C "$WT" add -A
    git -C "$WT" commit -q -m "chore: apply remaining changes (#$N)" -m "Co-Authored-By: Claude <noreply@anthropic.com>"
  fi
  if git -C "$WT" diff --name-only "origin/$BASE...HEAD" | grep -Eq '^(\.github/workflows/|scripts/agent/)'; then
    die "the change touches .github/workflows/ or scripts/agent/, which the agent may not edit; do that part by hand."
  fi
}

push() {
  git -C "$WT" push -q -u origin "$BRANCH"
}

summary_of() { # the agent's closing summary, without the result marker
  grep -v '^AGENT_RESULT:' "$LOG_DIR/$1.log" | tail -n 60
}

# --- worktree ----------------------------------------------------------------------------------

if [ ! -d "$REPO_DIR/.git" ]; then
  mkdir -p "$(dirname "$REPO_DIR")"
  gh repo clone "$REPO" "$REPO_DIR" -- -q
fi
git -C "$REPO_DIR" config credential.helper '!gh auth git-credential'
git -C "$REPO_DIR" config user.name "${AGENT_GIT_NAME:-twig-agent}"
git -C "$REPO_DIR" config user.email "${AGENT_GIT_EMAIL:-twig-agent@users.noreply.github.com}"
git -C "$REPO_DIR" fetch -q --prune origin
git -C "$REPO_DIR" worktree remove --force "$WT" >/dev/null 2>&1 || rm -rf "$WT"
git -C "$REPO_DIR" worktree prune

set_state doing
say "🤖 **agent:doing** — started. Run: ${RUN_URL:-n/a}"

CONFLICT_NOTE=
if git -C "$REPO_DIR" rev-parse -q --verify "origin/$BRANCH" >/dev/null; then
  git -C "$REPO_DIR" worktree add -q -B "$BRANCH" "$WT" "origin/$BRANCH"
  PR=$(find_pr)
  if ! git -C "$WT" merge -q --no-edit "origin/$BASE" >/dev/null 2>&1; then
    CONFLICT_NOTE=$'\n\n## Merge state\n\nThe branch conflicts with origin/'"$BASE"$': `git merge origin/'"$BASE"$'` is in progress in the worktree. Resolve the conflicts and commit the merge.\n'
  fi
else
  git -C "$REPO_DIR" worktree add -q -B "$BRANCH" "$WT" "origin/$BASE"
fi

# --- implement ---------------------------------------------------------------------------------

build_context >"$LOG_DIR/context.md"
printf '%s' "$CONFLICT_NOTE" >>"$LOG_DIR/context.md"
head_before=$(git -C "$WT" rev-parse HEAD)
implement implement implement.md "$LOG_DIR/context.md"

if [ "$(git -C "$WT" rev-parse HEAD)" = "$head_before" ] && [ -z "$PR" ]; then
  die "the agent finished without changing anything."
fi
push

if [ -z "$PR" ]; then
  title=$(git -C "$WT" log --reverse --format=%s "origin/$BASE..HEAD" | head -n 1)
  PR=$(gh pr create --repo "$REPO" --base "$BASE" --head "$BRANCH" --title "$title" \
    --body "Closes #$N"$'\n\n'"$(summary_of implement)"$'\n\n---\n'"Opened by the twig agent ([run]($RUN_URL))." |
    grep -Eo '[0-9]+$')
  gh pr edit "$PR" --repo "$REPO" --add-label "agent:doing" >/dev/null 2>&1 || true
  say "🔀 PR #$PR opened; reviewing."
fi

# --- review loop -------------------------------------------------------------------------------

for round in $(seq 1 "$MAX_REVIEW_ROUNDS"); do
  { cat "$SCRIPT_DIR/prompts/review.md"; printf '\n---\n'; cat "$LOG_DIR/context.md"; } |
    claude_run "review-$round" "$REVIEW_TOOLS" --disallowedTools 'Edit,Write' >/dev/null
  verdict=$(grep -Eo '^VERDICT: (APPROVE|CHANGES)' "$LOG_DIR/review-$round.log" | tail -n 1 || true)
  [ -n "$verdict" ] || die "the reviewer gave no verdict (round $round); see the run logs."

  { echo "$MARK"$'\n'"<!-- twig-agent:review -->"; echo "### Agent review, round $round"; echo; grep -v '^VERDICT:' "$LOG_DIR/review-$round.log"; echo; echo "**$verdict**"; } |
    gh pr comment "$PR" --repo "$REPO" --body-file - >/dev/null

  [ "$verdict" = "VERDICT: CHANGES" ] || break
  [ "$round" -lt "$MAX_REVIEW_ROUNDS" ] || die "the review still asks for changes after $MAX_REVIEW_ROUNDS rounds (see PR #$PR)."

  { cat "$LOG_DIR/context.md"; printf '\n## Code review findings to fix\n\n'; grep -v '^VERDICT:' "$LOG_DIR/review-$round.log"; } >"$LOG_DIR/fix-$round.md"
  implement "fix-review-$round" fix.md "$LOG_DIR/fix-$round.md"
  push
done

# --- PR gates ----------------------------------------------------------------------------------

wait_gates() { # 0 = all green, 1 = failed or timed out; sets GATE_REASON
  local deadline=$((SECONDS + CI_TIMEOUT)) sha json
  sha=$(git -C "$WT" rev-parse HEAD)
  GATE_REASON=
  sleep 45
  while [ "$SECONDS" -lt "$deadline" ]; do
    if [ "$(gh pr view "$PR" --repo "$REPO" --json headRefOid --jq .headRefOid)" = "$sha" ]; then
      json=$(gh pr checks "$PR" --repo "$REPO" --json name,bucket 2>/dev/null) || true
      [ -n "$json" ] || json='[]'
      if [ "$(jq 'length' <<<"$json")" -gt 0 ]; then
        if jq -e 'any(.[]; .bucket == "fail" or .bucket == "cancel")' <<<"$json" >/dev/null; then
          GATE_REASON=$(jq -r '[.[] | select(.bucket == "fail" or .bucket == "cancel") | .name] | join(", ")' <<<"$json")
          return 1
        fi
        jq -e 'any(.[]; .bucket == "pending")' <<<"$json" >/dev/null || return 0
      elif [ "$SECONDS" -gt $((deadline - CI_TIMEOUT + 300)) ]; then
        GATE_REASON="no checks started within 5 minutes (is AGENT_PAT a PAT, not github.token?)"
        return 1
      fi
    fi
    sleep 30
  done
  GATE_REASON="the gates did not finish within $((CI_TIMEOUT / 60)) minutes"
  return 1
}

failed_logs() {
  local sha id; sha=$(git -C "$WT" rev-parse HEAD)
  for id in $(gh run list --repo "$REPO" --branch "$BRANCH" --commit "$sha" --json databaseId,conclusion \
    --jq '.[] | select(.conclusion == "failure") | .databaseId'); do
    echo "### failing run $id"
    gh run view "$id" --repo "$REPO" --log-failed 2>&1 | tail -n 200
  done
}

for round in $(seq 1 "$MAX_CI_ROUNDS"); do
  wait_gates && break
  logs=$(failed_logs)
  [ -n "$logs" ] || die "gates failed ($GATE_REASON)."
  [ "$round" -lt "$MAX_CI_ROUNDS" ] || die "gates still failing after $MAX_CI_ROUNDS fix rounds: $GATE_REASON (PR #$PR)."
  say "🔧 Gates failed ($GATE_REASON); fixing (round $round)."
  { cat "$LOG_DIR/context.md"; printf '\n## Failing CI logs to fix\n\n%s\n' "$logs"; } >"$LOG_DIR/ci-$round.md"
  implement "fix-ci-$round" fix.md "$LOG_DIR/ci-$round.md"
  push
done

# --- done --------------------------------------------------------------------------------------

set_state done
say "✅ **agent:done** — PR #$PR is green and reviewed. Your turn: review and merge."
