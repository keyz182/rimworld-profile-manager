#!/usr/bin/env bash
# Updates the two AUR packages (rwpm-bin, rwpm) to a released version.
# usage: aur.sh <version> [--push]
#   Without --push: writes the finished package files to a work folder and prints it.
#   With --push:    also commits them to each package's AUR repo and pushes to master.
#                   The caller sets GIT_SSH_COMMAND (key and known hosts) and GH_TOKEN.
# The version must be released already: the checksums come from its GitHub assets.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO="keyz182/rimworld-profile-manager"
version="${1:?usage: aur.sh <version> [--push]}"
push="${2:-}"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || {
  echo "aur.sh: bad version '$version'" >&2
  exit 2
}
[[ -z "$push" || "$push" == "--push" ]] || {
  echo "aur.sh: unknown option '$push'" >&2
  exit 2
}

work="$(mktemp -d)"
echo "[aur] work folder: $work" >&2

# sha256 of the Linux release tarball (rwpm-bin) and of the tag's source archive (rwpm).
gh release download "v$version" -R "$REPO" -D "$work" -p "rwpm-$version-linux-x86_64.tar.gz"
bin_sha="$(sha256sum "$work/rwpm-$version-linux-x86_64.tar.gz" | cut -d' ' -f1)"
src_sha="$(curl -fsSL "https://github.com/$REPO/archive/refs/tags/v$version.tar.gz" | sha256sum | cut -d' ' -f1)"

# prepare <package> <sha256>: copies the template, sets version and checksum, makes .SRCINFO.
prepare() {
  local pkg="$1" sha="$2" dir="$work/$1"
  mkdir -p "$dir"
  cp "$ROOT/packaging/aur/$pkg/PKGBUILD" "$ROOT/packaging/aur/LICENSE" "$dir/"
  sed -i -e "s/^pkgver=.*/pkgver=$version/" -e "s/^pkgrel=.*/pkgrel=1/" \
    -e "s/^sha256sums=.*/sha256sums=('$sha')/" "$dir/PKGBUILD"
  # makepkg refuses to run as root, so run it as the calling user.
  docker run --rm --user "$(id -u):$(id -g)" -e HOME=/tmp -v "$dir:/pkg" -w /pkg \
    archlinux:base-devel makepkg --printsrcinfo >"$dir/.SRCINFO"
  grep -Eq "^[[:space:]]*pkgver = $version\$" "$dir/.SRCINFO" || {
    echo "aur.sh: $pkg .SRCINFO has the wrong version" >&2
    exit 1
  }
}

# publish <package>: commits the files to the package's AUR repo and pushes.
publish() {
  local pkg="$1" repo="$work/aur-$1"
  git -c init.defaultBranch=master clone -q "ssh://aur@aur.archlinux.org/$pkg.git" "$repo"
  cp "$work/$pkg/PKGBUILD" "$work/$pkg/.SRCINFO" "$work/$pkg/LICENSE" "$repo/"
  git -C "$repo" add PKGBUILD .SRCINFO LICENSE
  if git -C "$repo" diff --cached --quiet; then
    echo "[aur] $pkg is already at $version" >&2
    return
  fi
  git -C "$repo" -c user.name="Kieran David Evans" -c user.email="keyz182@gmail.com" \
    commit -q -m "Update to $version"
  git -C "$repo" push -q origin HEAD:master
  echo "[aur] pushed $pkg $version" >&2
}

prepare rwpm-bin "$bin_sha"
prepare rwpm "$src_sha"
if [[ "$push" == "--push" ]]; then
  publish rwpm-bin
  publish rwpm
else
  echo "[aur] dry run, nothing pushed. Files are in $work/rwpm-bin and $work/rwpm" >&2
fi
