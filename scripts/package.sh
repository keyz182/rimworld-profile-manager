#!/usr/bin/env bash
# Builds a release bundle (T19): rwpm, payload/, doorstop/ and every licence text.
# usage: package.sh linux|windows
# out:   target/dist/rwpm-<version>-<os>-x86_64.tar.gz (linux) | .zip (windows)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VENDOR="$ROOT/vendor/unitydoorstop/4.5.0"
# B4: Linux builds run in Valve's soldier SDK (Debian 10, glibc 2.28) so rwpm starts on
# older distros and inside Steam's soldier runtime container. Pinned by digest.
SOLDIER_SDK="registry.gitlab.steamos.cloud/steamrt/soldier/sdk@sha256:f87b72ad00e2eafa93e5c1c717d3bf3e7756e0a800a3599eec999d626b21b4c9"
GLIBC_FLOOR="2.28"
os="${1:?usage: package.sh linux|windows}"

case "$os" in
linux)
  target="x86_64-unknown-linux-gnu"
  exe="rwpm"
  loader="linux_x64/libdoorstop.so"
  ;;
windows)
  target="x86_64-pc-windows-gnu"
  exe="rwpm.exe"
  loader="win_x64/winhttp.dll"
  rustup target list --installed | grep -qx "$target" || {
    echo "package.sh: no Rust target $target; see T19 notes for the Windows toolchain" >&2
    exit 1
  }
  ;;
*)
  echo "package.sh: unknown os '$os' (linux|windows)" >&2
  exit 64
  ;;
esac

version="$(cargo metadata --no-deps --format-version 1 --manifest-path "$ROOT/Cargo.toml" |
  python3 -I -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"]=="rwpm"))')"
name="rwpm-$version-$os-x86_64"
out="$ROOT/target/dist/$name"
[[ ! -e "$out" ]] || trash "$out"
mkdir -p "$out/payload" "$out/doorstop" "$out/licenses/crates"

# rustup and crates are cached in target/soldier-cache; the toolchain is the pinned one.
build_linux() {
  local cache="$ROOT/target/soldier-cache"
  mkdir -p "$cache/home"
  docker run --rm --user "$(id -u):$(id -g)" -e HOME=/cache/home \
    -e CARGO_HOME=/cache/home/.cargo -e RUSTUP_HOME=/cache/home/.rustup \
    -v "$ROOT:/src:ro" -v "$cache:/cache" -v "$ROOT/target:/target" -w /src "$SOLDIER_SDK" \
    bash -c 'set -euo pipefail
      [[ -x "$CARGO_HOME/bin/rustup" ]] ||
        curl -sSf https://sh.rustup.rs | sh -s -- -y -q --profile minimal --default-toolchain none --no-modify-path
      "$CARGO_HOME/bin/cargo" build -q --locked --release -p rwpm --target '"$target"' --target-dir /target/soldier'
}

# Fails when the binary needs a newer glibc than the floor.
check_glibc() {
  local need
  need="$(objdump -T "$1" | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -uV | tail -1)"
  [[ "$(printf '%s\n%s\n' "$need" "$GLIBC_FLOOR" | sort -V | tail -1)" == "$GLIBC_FLOOR" ]] || {
    echo "package.sh: rwpm needs glibc $need, above the $GLIBC_FLOOR floor (B4)" >&2
    exit 1
  }
  echo "[package] needs glibc $need (floor $GLIBC_FLOOR)" >&2
}

echo "[package] rwpm $version for $target" >&2
if [[ "$os" == linux ]]; then
  build_linux
  check_glibc "$ROOT/target/soldier/$target/release/$exe"
  cp "$ROOT/target/soldier/$target/release/$exe" "$out/"
else
  cargo build -q --release -p rwpm --target "$target" --manifest-path "$ROOT/Cargo.toml"
  cp "$ROOT/target/$target/release/$exe" "$out/"
fi

dotnet build "$ROOT/payload/Rwpm.Payload" -c Release -o "$ROOT/target/dist/payload-build" -nologo >/dev/null
cp "$ROOT/target/dist/payload-build/Rwpm.Payload.dll" "$out/payload/"

(cd "$VENDOR" && sha256sum -c --quiet SHA256SUMS)
cp "$VENDOR/$loader" "$out/doorstop/"

cp "$ROOT/LICENSE" "$out/LICENSE"
cp "$VENDOR/LICENSE" "$out/licenses/UnityDoorstop-LGPL-2.1.txt"
# Embedded fonts: Inter (OFL 1.1), Font Awesome Free (icons CC BY 4.0, font OFL 1.1).
cp "$ROOT"/licenses/*.txt "$out/licenses/"
# App icon and the desktop menu entry installer (Linux; Windows has it inside rwpm.exe).
if [[ "$os" == linux ]]; then
  mkdir -p "$out/icons"
  cp "$ROOT"/crates/rwpm/assets/icon/rwpm-*.png "$ROOT/crates/rwpm/assets/icon/rwpm.svg" "$out/icons/"
  cp "$ROOT/packaging/linux/install-desktop-entry.sh" "$out/"
fi
cargo metadata --format-version 1 --filter-platform "$target" --manifest-path "$ROOT/Cargo.toml" |
  python3 -I "$ROOT/scripts/third_party.py" "$out/licenses" "$ROOT/vendor/licenses"

case "$os" in
linux) tar -C "$out/.." -czf "$out.tar.gz" "$name" && echo "$out.tar.gz" ;;
windows) (cd "$out/.." && zip -qr "$out.zip" "$name") && echo "$out.zip" ;;
esac
