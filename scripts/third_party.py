"""Copies the licence files of every crate shipped inside rwpm into a bundle.

Reads `cargo metadata` (with --filter-platform) on stdin. Follows only normal
dependencies from the `rwpm` and `rwpm-steam` binaries (build and dev dependencies are not shipped),
copies each crate's LICENSE/COPYING/NOTICE files and writes THIRD-PARTY.md.
Exits 1 if a shipped crate has no licence file, so nothing ships unlicensed.

Crates published without a licence file get theirs from vendor/licenses/ (copied
from the crate's own repository), mapped in FROM_REPO below.

usage: cargo metadata ... | python3 -I third_party.py <bundle>/licenses <repo>/vendor/licenses
"""

import json
import pathlib
import shutil
import sys

# Both programs in a bundle. rwpm-steam is the Steam helper.
SHIPPED_BINARIES = ("rwpm", "rwpm-steam")
LICENCE_PREFIXES = ("license", "licence", "copying", "notice", "unlicense")
LICENCE_DIRS = ("licenses", "licences", "license")

# Crate -> folder in vendor/licenses holding its repository's licence files.
FROM_REPO = {
    "clipboard-win": "DoumanAsh_clipboard-win",
    "gpu-alloc": "zakarumych_gpu-alloc",
    "gpu-alloc-types": "zakarumych_gpu-alloc",
    "gpu-descriptor": "zakarumych_gpu-descriptor",
    "gpu-descriptor-types": "zakarumych_gpu-descriptor",
    "hexf-parse": "lifthrasiir_hexf",
    "profiling": "aclysma_profiling",
    "spirv": "gfx-rs_rspirv",
    "svg_fmt": "nical_rust_debug",
    # Published without a licence file; the same repository as `steamworks`.
    "steamworks-sys": "Noxime_steamworks-rs",
    "winapi-x86_64-pc-windows-gnu": "retep998_winapi-rs",
    **{
        f"iced_{n}": "iced-rs_iced"
        for n in (
            "core",
            "debug",
            "futures",
            "graphics",
            "program",
            "renderer",
            "runtime",
            "tiny_skia",
            "wgpu",
            "widget",
            "winit",
        )
    },
    # macOS-only transitive deps, pulled in by iced's Cocoa backend (winit/wgpu).
    "block": "SSheldon_rust-block",
    "dispatch": "SSheldon_rust-dispatch",
    "malloc_buf": "SSheldon_malloc_buf",
    **{
        n: "madsmtm_objc2"
        for n in (
            "block2",
            "dispatch2",
            "objc-sys",
            "objc2",
            "objc2-app-kit",
            "objc2-cloud-kit",
            "objc2-core-data",
            "objc2-core-foundation",
            "objc2-core-graphics",
            "objc2-core-image",
            "objc2-core-text",
            "objc2-core-video",
            "objc2-encode",
            "objc2-foundation",
            "objc2-io-kit",
            "objc2-io-surface",
            "objc2-metal",
            "objc2-quartz-core",
        )
    },
}


def licence_files(src: pathlib.Path) -> list[pathlib.Path]:
    """Top-level licence files plus everything in a LICENSES/-style folder."""
    found = []
    for f in src.iterdir():
        name = f.name.lower()
        if f.is_file() and name.startswith(LICENCE_PREFIXES):
            found.append(f)
        elif f.is_dir() and name in LICENCE_DIRS:
            found.extend(g for g in f.iterdir() if g.is_file())
    return found


def shipped(meta: dict) -> set[str]:
    """Package ids reachable from rwpm or rwpm-steam through normal dependencies."""
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    roots = [p["id"] for p in meta["packages"] if p["name"] in SHIPPED_BINARIES]
    seen, todo = set(), roots
    while todo:
        pid = todo.pop()
        if pid in seen:
            continue
        seen.add(pid)
        for dep in nodes[pid]["deps"]:
            if any(k["kind"] is None for k in dep["dep_kinds"]):
                todo.append(dep["pkg"])
    return seen


def main() -> None:
    out, vendor = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
    meta = json.load(sys.stdin)
    members = set(meta["workspace_members"])
    packages = {p["id"]: p for p in meta["packages"]}
    rows, missing = [], []
    for pid in sorted(shipped(meta) - members, key=lambda i: packages[i]["name"]):
        pkg = packages[pid]
        src = pathlib.Path(pkg["manifest_path"]).parent
        files = licence_files(src)
        if not files and pkg["name"] in FROM_REPO:
            files = licence_files(vendor / FROM_REPO[pkg["name"]])
        if pkg.get("license_file"):
            files.append(src / pkg["license_file"])
        dest = out / "crates" / f"{pkg['name']}-{pkg['version']}"
        dest.mkdir(parents=True, exist_ok=True)
        for f in set(files):
            if f.is_file():
                shutil.copy(f, dest / f.name)
        if not any(dest.iterdir()):
            missing.append(f"{pkg['name']} {pkg['version']}")
        rows.append(
            f"| {pkg['name']} | {pkg['version']} | {pkg.get('license') or 'see files'} | {pkg.get('repository') or ''} |"
        )
    (out / "THIRD-PARTY.md").write_text(
        "# Third-party software in RimWorld Profile Manager\n\n"
        "Licence texts for each crate are in `crates/<name>-<version>/`. "
        "UnityDoorstop (LGPL-2.1) is in `UnityDoorstop-LGPL-2.1.txt`; its source is at "
        "https://github.com/NeighTools/UnityDoorstop (tag v4.5.0).\n\n"
        "| crate | version | licence | source |\n|---|---|---|---|\n"
        + "\n".join(rows)
        + "\n"
    )
    print(f"[package] {len(rows)} crates, licence files copied", file=sys.stderr)
    if missing:
        print(
            "[package] no licence file shipped in: " + ", ".join(missing),
            file=sys.stderr,
        )
        sys.exit(1)


main()
