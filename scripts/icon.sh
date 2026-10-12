#!/usr/bin/env bash
# Generates the app icon files from crates/rwpm/assets/icon/rwpm.svg (the master copy):
# PNGs for Linux, rwpm.ico for the Windows .exe, and raw RGBA for the window icon.
# Needs ImageMagick 7 (`magick`). Run after changing the SVG; commit the outputs.
set -euo pipefail

dir="$(cd "$(dirname "$0")/.." && pwd)/crates/rwpm/assets/icon"
svg="$dir/rwpm.svg"

render() { # size output
  magick -background none -density 384 "$svg" -resize "$1x$1" "$2"
}

for n in 16 32 48 64 128 256; do
  render "$n" "$dir/rwpm-$n.png"
done
magick "$dir"/rwpm-{16,32,48,256}.png "$dir/rwpm.ico"
# 64 px raw RGBA for iced's window::icon::from_rgba (no PNG decoder needed at run time).
magick "$dir/rwpm-64.png" -depth 8 "RGBA:$dir/rwpm-64.rgba"
echo "icon files written to $dir"
