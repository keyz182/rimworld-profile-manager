#!/usr/bin/env bash
# Builds the .deb and .rpm from an unpacked Linux bundle (made by package.sh linux).
# usage: deb-rpm.sh <target/dist/rwpm-<version>-linux-x86_64>
# out:   target/dist/rwpm_<version>-1_amd64.deb and target/dist/rwpm-<version>-1.x86_64.rpm
# Nothing is rebuilt: the packages hold the files of the bundle, so they keep its glibc floor.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# Pinned by version and checksum; downloaded once into target/nfpm-tools.
TOOL_VERSION="2.47.0"
TOOL_URL="https://github.com/goreleaser/nfpm/releases/download/v$TOOL_VERSION/nfpm_${TOOL_VERSION}_Linux_x86_64.tar.gz"
TOOL_SHA="0660ca602b2d2d2ae4781a06c692b3eeb9d437ffea05b831d76e41f4a3188783"

src="$(cd "${1:?usage: deb-rpm.sh <unpacked linux bundle dir>}" && pwd)"
name="$(basename "$src")"
[[ "$name" =~ ^rwpm-([0-9]+\.[0-9]+\.[0-9]+)-linux-x86_64$ ]] || {
  echo "deb-rpm.sh: '$name' is not a Linux bundle folder" >&2
  exit 64
}
version="${BASH_REMATCH[1]}"
out="$(dirname "$src")"

tools="$ROOT/target/nfpm-tools"
mkdir -p "$tools"
tarball="$tools/nfpm-$TOOL_VERSION.tar.gz"
if ! { [[ -f "$tarball" ]] && echo "$TOOL_SHA  $tarball" | sha256sum -c --quiet 2>/dev/null; }; then
  curl -sSfL -o "$tarball.part" "$TOOL_URL"
  echo "$TOOL_SHA  $tarball.part" | sha256sum -c --quiet
  mv "$tarball.part" "$tarball"
fi
tar -xzf "$tarball" -C "$tools" nfpm

# nfpm reads src paths relative to its working folder, so stage the bundle plus the
# desktop entry (which is not in the bundle) in a folder this script owns under target/.
stage="$ROOT/target/deb-rpm/$name"
# `trash` is a local convenience, absent on the CI runners; $stage is a folder this script owns.
if [[ -e "$stage" ]]; then
  if command -v trash >/dev/null 2>&1; then trash "$stage"; else rm -rf "$stage"; fi
fi
mkdir -p "$(dirname "$stage")"
cp -a "$src" "$stage"
cp "$ROOT/packaging/linux/rwpm.desktop" "$stage/rwpm.desktop"

cd "$stage"
for packager in deb rpm; do
  VERSION="$version" "$tools/nfpm" package --config "$ROOT/packaging/linux/nfpm.yaml" \
    --packager "$packager" --target "$out/"
done
ls "$out"/rwpm_"$version"-1_amd64.deb "$out"/rwpm-"$version"-1.x86_64.rpm
