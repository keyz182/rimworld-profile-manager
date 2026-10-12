#!/usr/bin/env bash
# Renders the app's screens headlessly (light theme) into a wiki checkout's images/ folder.
# usage: wiki-shots.sh <path to a clone of the repository's .wiki.git>
# Names drop the test's number prefix: 01-main-default → images/main-default.png.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
wiki="$(cd "${1:?usage: wiki-shots.sh <wiki checkout>}" && pwd)"
[[ -f "$wiki/Home.md" ]] || {
  echo "wiki-shots.sh: $wiki has no Home.md; is it the wiki checkout?" >&2
  exit 64
}

shots="$ROOT/target/wiki-shots"
mkdir -p "$shots"
RWPM_SNAPSHOT_DIR="$shots" cargo test -q -p rwpm --locked --manifest-path "$ROOT/Cargo.toml" \
  render_screens_manual -- --ignored >&2
mkdir -p "$wiki/images"
for f in "$shots"/*-light-wgpu.png; do
  name="$(basename "$f" -light-wgpu.png)"
  cp "$f" "$wiki/images/${name#[0-9][0-9]-}.png"
done
echo "$wiki/images"
