#!/usr/bin/env bash
# Builds a release bundle (T19, T46): rwpm, payload/, doorstop/ and every licence text.
# usage: package.sh linux|windows|macos
# out:   target/dist/rwpm-<version>-<os>-x86_64.tar.gz (linux) | .zip (windows, macos)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VENDOR="$ROOT/vendor/unitydoorstop/4.5.0"
# B4: Linux builds run in Valve's soldier SDK (Debian 10, glibc 2.28) so rwpm starts on
# older distros and inside Steam's soldier runtime container. Pinned by digest.
SOLDIER_SDK="registry.gitlab.steamos.cloud/steamrt/soldier/sdk@sha256:f87b72ad00e2eafa93e5c1c717d3bf3e7756e0a800a3599eec999d626b21b4c9"
GLIBC_FLOOR="2.28"
os="${1:?usage: package.sh linux|windows|macos}"

# `sha256sum` is not on every macOS: it arrived with macOS 26, and the macos-14 CI
# runner has no such command (exit 127). `shasum` ships with every macOS and with the
# Linux runners, and reads the same `<hash>  <path>` format.
sha256_check() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c --quiet "$1"
  else
    shasum -a 256 -c "$1" >/dev/null
  fi
}

# `trash` is a local convenience, absent on the CI runners. `$1` is always a build
# output directory this script owns under target/, so plain removal is safe there.
discard() {
  if command -v trash >/dev/null 2>&1; then
    trash "$1"
  else
    rm -rf "$1"
  fi
}

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
macos)
  # T46/V42: arm64 only — x86_64-apple-darwin isn't installed, and a universal binary
  # costs a second target + lipo + an x86 CI leg for zero present Intel-Mac users.
  target="aarch64-apple-darwin"
  exe="rwpm"
  loader="macos/libdoorstop.dylib"
  rustup target list --installed | grep -qx "$target" || {
    echo "package.sh: no Rust target $target" >&2
    exit 1
  }
  ;;
*)
  echo "package.sh: unknown os '$os' (linux|windows|macos)" >&2
  exit 64
  ;;
esac

version="$(cargo metadata --no-deps --format-version 1 --manifest-path "$ROOT/Cargo.toml" |
  python3 -I -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"]=="rwpm"))')"
arch="x86_64"
[[ "$os" != macos ]] || arch="arm64"
name="rwpm-$version-$os-$arch"
out="$ROOT/target/dist/$name"
[[ ! -e "$out" ]] || discard "$out"
# T46: macOS ships an .app bundle. `PayloadFiles::beside(app_dir)` resolves payload/ and
# doorstop/ as siblings of the exe (launch.rs/run.rs — unchanged for macOS), so the exe,
# payload/ and doorstop/ all live in Contents/MacOS/ together, exactly like the flat
# linux/windows layout. licenses/ goes in Resources/ (not read by anything at runtime).
if [[ "$os" == macos ]]; then
  bundle="$out/RimWorld Profile Manager.app"
  bin_dir="$bundle/Contents/MacOS"
  license_dir="$bundle/Contents/Resources/licenses"
  root_file_dir="$bundle/Contents/Resources"
else
  bundle=""
  bin_dir="$out"
  license_dir="$out/licenses"
  root_file_dir="$out"
fi
mkdir -p "$bin_dir/payload" "$bin_dir/doorstop" "$license_dir/crates"

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
  cp "$ROOT/target/soldier/$target/release/$exe" "$bin_dir/"
else
  cargo build -q --release -p rwpm --target "$target" --manifest-path "$ROOT/Cargo.toml"
  cp "$ROOT/target/$target/release/$exe" "$bin_dir/"
fi

dotnet build "$ROOT/payload/Rwpm.Payload" -c Release -o "$ROOT/target/dist/payload-build" -nologo >/dev/null
cp "$ROOT/target/dist/payload-build/Rwpm.Payload.dll" "$bin_dir/payload/"
# T64: the RimWorld 1.7 startup hook ships in every bundle. The launcher copy needs it
# (flatpak::sync_launcher), so a bundle without it could not launch any profile.
dotnet build "$ROOT/payload/Rwpm.Payload.Hook" -c Release -o "$ROOT/target/dist/hook-build" -nologo >/dev/null
cp "$ROOT/target/dist/hook-build/Rwpm.Payload.Hook.dll" "$bin_dir/payload/"

(cd "$VENDOR" && sha256_check SHA256SUMS)
cp "$VENDOR/$loader" "$bin_dir/doorstop/"

cp "$ROOT/LICENSE" "$root_file_dir/LICENSE"
cp "$VENDOR/LICENSE" "$license_dir/UnityDoorstop-LGPL-2.1.txt"
# Embedded fonts: Inter (OFL 1.1), Font Awesome Free (icons CC BY 4.0, font OFL 1.1).
cp "$ROOT"/licenses/*.txt "$license_dir/"
# App icon and the desktop menu entry installer (Linux; Windows has it inside rwpm.exe;
# macOS bundle icon is T46's follow-up — Info.plist works without one, falling back to
# the generic app icon).
if [[ "$os" == linux ]]; then
  mkdir -p "$out/icons"
  cp "$ROOT"/crates/rwpm/assets/icon/rwpm-*.png "$ROOT/crates/rwpm/assets/icon/rwpm.svg" "$out/icons/"
  cp "$ROOT/packaging/linux/install-desktop-entry.sh" "$ROOT/packaging/linux/rwpm-mime.xml" "$out/"
fi
cargo metadata --format-version 1 --filter-platform "$target" --manifest-path "$ROOT/Cargo.toml" |
  python3 -I "$ROOT/scripts/third_party.py" "$license_dir" "$ROOT/vendor/licenses"

# V42: Info.plist + adhoc codesign. `--deep` covers the vendored dylib and payload DLL
# too (codesign walks the whole bundle); neither needs its own signature to load under
# Mono/dyld (R24/R25 — only arm64 Mach-O images need one), but `--deep` signing them
# anyway costs nothing and keeps `codesign --verify --deep --strict` clean end to end.
if [[ "$os" == macos ]]; then
  cat >"$bundle/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>rwpm</string>
  <key>CFBundleIdentifier</key>
  <string>uk.dbyz.rimworld-profile-manager</string>
  <key>CFBundleName</key>
  <string>RimWorld Profile Manager</string>
  <key>CFBundleShortVersionString</key>
  <string>$version</string>
  <key>CFBundleVersion</key>
  <string>$version</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>LSMinimumSystemVersion</key>
  <string>11.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST
  codesign --force --deep --sign - "$bundle"
  codesign --verify --deep --strict "$bundle"
fi

case "$os" in
linux) tar -C "$out/.." -czf "$out.tar.gz" "$name" && echo "$out.tar.gz" ;;
windows) (cd "$out/.." && zip -qr "$out.zip" "$name") && echo "$out.zip" ;;
# ditto -c -k --keepParent preserves the bundle's code signature; a plain `zip` can
# reorder/strip resource forks and invalidate it.
macos) (cd "$out" && ditto -c -k --keepParent "RimWorld Profile Manager.app" "$out.zip") && echo "$out.zip" ;;
esac
