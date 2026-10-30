#!/usr/bin/env bash
# Packs a release build into dist/<name>.tar.gz (dist/<name>.zip on Windows).
#
# Usage: tools/ci/package.sh <target-triple> <name>
#   run from the repository root after `cargo build --release --target <target-triple>`.
#
# A shipped build needs its assets/ folder (maps, models, fonts) next to the executable; data/
# is optional (the same files are built in, a copy next to the executable overrides them).
set -euo pipefail

target=${1:?usage: package.sh <target-triple> <name>}
name=${2:?usage: package.sh <target-triple> <name>}

exe=frontline
[[ $target == *windows* ]] && exe=frontline.exe

bin="target/$target/release/$exe"
[ -f "$bin" ] || { echo "missing $bin: build with --release --target $target first" >&2; exit 1; }

stage="dist/$name"
rm -rf dist
mkdir -p "$stage"
cp "$bin" "$stage/"
cp -R crates/frontline/assets "$stage/assets"
cp -R data "$stage/data"
cp README.md "$stage/"

if [[ $target == *windows* ]]; then
  (cd dist && 7z a -tzip "$name.zip" "$name" > /dev/null)
  echo "dist/$name.zip"
else
  tar -C dist -czf "dist/$name.tar.gz" "$name"
  echo "dist/$name.tar.gz"
fi
