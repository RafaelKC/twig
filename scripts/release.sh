#!/bin/sh
# Cut a release in two steps, because main only accepts changes through a pull request.
#
#   scripts/release.sh 0.2.0          1. on main: branch release/v0.2.0, bump the version, run the
#                                        tests, commit "release v0.2.0", push the branch, open a PR
#   (merge the PR once `check` and `test-macos` pass)
#   scripts/release.sh --tag 0.2.0    2. on updated main: verify the release commit is there, create
#                                        the annotated tag v0.2.0 and push only the tag
#
# The tag triggers .github/workflows/release.yml, which builds, signs and publishes.
# The branch is release/vX.Y.Z (never vX.Y.Z) so it can't collide with the tag.
set -eu

usage() {
  echo "usage: scripts/release.sh X.Y.Z          (bump, test, push release/vX.Y.Z, open PR)" >&2
  echo "       scripts/release.sh --tag X.Y.Z    (after the PR is merged: tag main and push the tag)" >&2
  exit 1
}

mode=prepare
if [ "${1:-}" = "--tag" ]; then
  mode=tag
  shift
fi
[ $# -eq 1 ] || usage
version="$1"
case "$version" in
  [0-9]*.[0-9]*.[0-9]*) ;;
  *) usage ;;
esac
tag="v$version"
branch="release/$tag"

cd "$(dirname "$0")/.."

[ "$(git rev-parse --abbrev-ref HEAD)" = main ] || { echo "release from main" >&2; exit 1; }
[ -z "$(git status --porcelain)" ] || { echo "working tree is not clean" >&2; exit 1; }
git fetch --quiet origin
[ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] || { echo "main is not in sync with origin/main" >&2; exit 1; }
if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then echo "tag $tag already exists" >&2; exit 1; fi
if git ls-remote --exit-code --tags origin "refs/tags/$tag" >/dev/null 2>&1; then
  echo "tag $tag already exists on origin" >&2
  exit 1
fi

current=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)

if [ "$mode" = tag ]; then
  # the release commit ("release vX.Y.Z", possibly suffixed with "(#NN)" by a squash merge)
  # must be on main and must carry this version in Cargo.toml
  commit=$(git log --format=%H --extended-regexp --grep="^release $tag( \(#[0-9]+\))?\$" -n 1 HEAD)
  [ -n "$commit" ] || { echo "no 'release $tag' commit on main: merge the release PR first" >&2; exit 1; }
  at_commit=$(git show "$commit:Cargo.toml" | sed -n 's/^version = "\(.*\)"/\1/p' | head -n 1)
  [ "$at_commit" = "$version" ] || { echo "Cargo.toml at $commit is $at_commit, not $version" >&2; exit 1; }
  [ "$current" = "$version" ] || { echo "Cargo.toml on main is $current, not $version" >&2; exit 1; }

  git tag -a "$tag" -m "twig $tag" "$commit"
  printf "push tag %s (%s) to origin? [y/N] " "$tag" "$(git rev-parse --short "$commit")"
  read -r answer
  case "$answer" in
    y | Y) git push --quiet origin "refs/tags/$tag" && echo "pushed; follow the build in GitHub Actions" ;;
    *) echo "not pushed. To push: git push origin refs/tags/$tag   (to undo: git tag -d $tag)" ;;
  esac
  exit 0
fi

[ "$current" != "$version" ] || { echo "Cargo.toml is already at $version; if its release commit is on main, run: scripts/release.sh --tag $version" >&2; exit 1; }
if git rev-parse -q --verify "refs/heads/$branch" >/dev/null; then echo "branch $branch already exists locally" >&2; exit 1; fi
if git ls-remote --exit-code --heads origin "refs/heads/$branch" >/dev/null 2>&1; then
  echo "branch $branch already exists on origin" >&2
  exit 1
fi
command -v gh >/dev/null 2>&1 || { echo "gh (GitHub CLI) is required to open the PR" >&2; exit 1; }

echo "-> $current -> $version on $branch"
git switch --quiet -c "$branch"
# only the [package] version (the first `version = ` line)
awk -v v="$version" '!done && /^version = "/ { print "version = \"" v "\""; done = 1; next } { print }' \
  Cargo.toml >Cargo.toml.tmp && mv Cargo.toml.tmp Cargo.toml
cargo test --quiet # also refreshes Cargo.lock
git add Cargo.toml Cargo.lock
git commit --quiet -m "release $tag"

printf "push %s and open a PR to main? [y/N] " "$branch"
read -r answer
case "$answer" in
  y | Y)
    git push --quiet -u origin "$branch"
    gh pr create --base main --head "$branch" --title "release $tag" \
      --body "Version bump for $tag. After merging, on updated main run: \`scripts/release.sh --tag $version\`"
    echo "after the PR is merged: git switch main && git pull && scripts/release.sh --tag $version"
    ;;
  *) echo "not pushed. To push: git push -u origin $branch   (to undo: git switch main && git branch -D $branch)" ;;
esac
