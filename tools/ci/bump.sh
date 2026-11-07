#!/usr/bin/env bash
# Bumps the version of every crate (crates/frontline and crates/mapedit) together and refreshes
# Cargo.lock, so `tools/ci/release.sh vX.Y.Z` finds tag and crate version equal.
#
# Usage: tools/ci/bump.sh patch|minor|major     0.2.0 -> 0.2.1 | 0.3.0 | 1.0.0
#        tools/ci/bump.sh X.Y.Z[-suffix]        set it (0.3.0-rc1 makes a pre-release tag)
#        add --commit to commit just the three files ("Version X.Y.Z")
set -euo pipefail

arg=${1:?usage: bump.sh <patch|minor|major|X.Y.Z[-suffix]> [--commit]}
commit=${2:-}
[ -z "$commit" ] || [ "$commit" = --commit ] || { echo "unknown option: $commit" >&2; exit 1; }

cd "$(git rev-parse --show-toplevel)"
manifests=(crates/frontline/Cargo.toml crates/mapedit/Cargo.toml)
files=("${manifests[@]}" Cargo.lock)

current=$(awk -F'"' '/^version/ {print $2; exit}' crates/frontline/Cargo.toml)
[[ $current =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)(-.+)?$ ]] || { echo "cannot read the current version: $current" >&2; exit 1; }
major=${BASH_REMATCH[1]} minor=${BASH_REMATCH[2]} patch=${BASH_REMATCH[3]}

case "$arg" in
  patch) new="$major.$minor.$((patch + 1))" ;;
  minor) new="$major.$((minor + 1)).0" ;;
  major) new="$((major + 1)).0.0" ;;
  *)
    [[ $arg =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || { echo "not a version: $arg" >&2; exit 1; }
    new=$arg ;;
esac
[ "$new" != "$current" ] || { echo "already at $current" >&2; exit 1; }

if [ -n "$commit" ] && [ -n "$(git status --porcelain --untracked-files=no -- "${files[@]}")" ]; then
  echo "NOT READY: ${files[*]} have uncommitted changes" >&2
  exit 1
fi

for manifest in "${manifests[@]}"; do
  awk -v new="$new" '!done && /^version = / { print "version = \"" new "\""; done = 1; next } { print }' \
    "$manifest" > "$manifest.tmp"
  mv "$manifest.tmp" "$manifest"
done

# Only the workspace's own packages change in the lockfile; `--locked` then proves it is complete.
cargo update --workspace
cargo check --locked --workspace --quiet

echo "$current -> $new"
if [ "$commit" = --commit ]; then
  git add -- "${files[@]}"
  git commit -q -m "Version $new" -- "${files[@]}"
  echo "committed; push main, then: tools/ci/release.sh v$new"
else
  echo "next: commit and push main, then: tools/ci/release.sh v$new"
fi
