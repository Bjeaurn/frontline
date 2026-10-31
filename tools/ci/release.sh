#!/usr/bin/env bash
# Tags and pushes a release, after checking everything that would otherwise only fail (or hang)
# in GitHub Actions: a clean, pushed main; a tag that matches the crate version; the self-hosted
# Mac runner that builds macOS online; enough free disk for those builds.
#
# Usage: tools/ci/release.sh <vX.Y.Z>          tag and push
#        tools/ci/release.sh <vX.Y.Z> --check  run the checks only
set -euo pipefail

tag=${1:?usage: release.sh <vX.Y.Z> [--check]}
mode=${2:-}
repo=Bjeaurn/frontline
runner_label=frontline-release
min_free_gb=10

cd "$(git rev-parse --show-toplevel)"
fail() { echo "NOT READY: $*" >&2; exit 1; }

version=$(grep -m1 '^version' crates/frontline/Cargo.toml | cut -d'"' -f2)
[ "$tag" = "v$version" ] || fail "tag $tag does not match crates/frontline/Cargo.toml version $version"
git rev-parse -q --verify "refs/tags/$tag" > /dev/null && fail "tag $tag already exists"

[ "$(git branch --show-current)" = main ] || fail "not on main"
[ -z "$(git status --porcelain --untracked-files=no)" ] || fail "uncommitted changes"
git fetch -q origin main
[ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] || fail "main is not in sync with origin/main (push or pull first)"

# The macOS jobs wait for this runner; GitHub cancels a job that waits more than 24 hours.
status=$(gh api "repos/$repo/actions/runners" \
  --jq "[.runners[] | select(any(.labels[]; .name == \"$runner_label\"))] | map(.status) | first // \"none\"")
[ "$status" = online ] || fail "the Mac runner is $status: start it with ~/actions-runner-frontline/start.sh"

free_gb=$(df -g "$HOME" | awk 'NR==2 {print $4}')
[ "$free_gb" -ge "$min_free_gb" ] || fail "only ${free_gb} GB free, the macOS builds need about $min_free_gb GB"

echo "ready: $tag, main pushed, Mac runner online, ${free_gb} GB free"
[ "$mode" = --check ] && exit 0

git tag -a "$tag" -m "Frontline $tag"
git push origin "$tag"
echo "pushed $tag: keep the runner running until the release is published"
echo "https://github.com/$repo/actions/workflows/release.yml"
