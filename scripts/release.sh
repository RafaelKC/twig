#!/bin/sh
# Cut a release: bump the version, run the tests, commit, tag and push.
# The tag triggers .github/workflows/release.yml, which builds, signs and publishes.
#
#   scripts/release.sh 0.2.0
set -eu

version="${1:-}"
case "$version" in
  [0-9]*.[0-9]*.[0-9]*) ;;
  *) echo "usage: scripts/release.sh X.Y.Z" >&2; exit 1 ;;
esac
tag="v$version"

cd "$(dirname "$0")/.."

[ "$(git rev-parse --abbrev-ref HEAD)" = main ] || { echo "release from main" >&2; exit 1; }
[ -z "$(git status --porcelain)" ] || { echo "working tree is not clean" >&2; exit 1; }
git fetch --quiet origin
[ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] || { echo "main is not in sync with origin/main" >&2; exit 1; }
if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then echo "tag $tag already exists" >&2; exit 1; fi

current=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)

if [ "$current" = "$version" ]; then
  # already at this version (e.g. the first release): just test and tag
  echo "→ $version (no bump)"
  cargo test --quiet
  undo="git tag -d $tag"
else
  echo "→ $current → $version"
  # only the [package] version (the first `version = ` line)
  awk -v v="$version" '!done && /^version = "/ { print "version = \"" v "\""; done = 1; next } { print }' \
    Cargo.toml >Cargo.toml.tmp && mv Cargo.toml.tmp Cargo.toml
  cargo test --quiet # also refreshes Cargo.lock
  git add Cargo.toml Cargo.lock
  git commit --quiet -m "release $tag"
  undo="git tag -d $tag && git reset --hard HEAD~1"
fi
git tag -a "$tag" -m "twig $tag"

printf "push main and %s to origin? [y/N] " "$tag"
read -r answer
case "$answer" in
  y | Y) git push --quiet origin main "$tag" && echo "✔ pushed; follow the build in GitHub Actions" ;;
  *) echo "not pushed. To push: git push origin main $tag   (to undo: $undo)" ;;
esac
