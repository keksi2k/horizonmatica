#!/usr/bin/env python3
"""Packs the bundle into horizonmatica-<version>.hnmod (a zip: horizon-native.json + mixins/horizonmatica/...).

    python tools/package.py [--horizon <commit>]

--horizon overrides the pinned commit (default: [compatibility] horizon_revision of horizon-mixin.toml).
"""
import argparse
import json
import pathlib
import re
import zipfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
BUNDLE = ROOT / "mixins" / "horizonmatica"
MIXINS_REV = "a2af5c0dd4117d51ecd70066ba7a00b4a2c10dab"  # launcher's native_mods::MIXINS_REV
ALLOWED_TEXT = ("README", "LICENSE", "COPYING", "NOTICE", ".gitignore")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--horizon", help="Horizon commit to pin (40 hex chars)")
    args = ap.parse_args()
    # The bundle's copy of horizon-menus must match the library (the libs/horizon-menus submodule).
    import subprocess
    import sys
    vendor = ROOT / "libs" / "horizon-menus" / "tools" / "vendor_menus.py"
    if vendor.exists():
        subprocess.run([sys.executable, str(vendor), str(BUNDLE), "horizonmatica", "--check"], check=True)
    else:
        print("libs/horizon-menus is missing (git submodule update --init): the menu copy is not checked")
    toml = (BUNDLE / "horizon-mixin.toml").read_text(encoding="utf-8")
    version = re.search(r'^version\s*=\s*"([^"]+)"', toml, re.M).group(1)
    pinned = re.search(r'^horizon_revision\s*=\s*"([0-9a-f]{40})"', toml, re.M).group(1)
    horizon = args.horizon or pinned
    if not re.fullmatch(r"[0-9a-f]{40}", horizon):
        raise SystemExit("--horizon must be a 40-character commit hash")
    if horizon != pinned:
        toml = toml.replace(pinned, horizon)

    manifest = {"schema": 1, "id": "horizonmatica", "version": version, "game": "26.3",
                "horizon": horizon, "mixins": MIXINS_REV}
    out = ROOT / "dist" / f"horizonmatica-{version}.hnmod"
    out.parent.mkdir(exist_ok=True)
    files = 0
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("horizon-native.json", json.dumps(manifest, indent=2) + "\n")
        for path in sorted(BUNDLE.rglob("*")):
            if not path.is_file() or "target" in path.relative_to(BUNDLE).parts:
                continue
            rel = path.relative_to(BUNDLE)
            if path.suffix not in (".rs", ".toml") and not path.name.startswith(ALLOWED_TEXT) \
                    and "data" not in rel.parts:
                continue
            arc = f"mixins/horizonmatica/{rel.as_posix()}"
            if rel.as_posix() == "horizon-mixin.toml":
                z.writestr(arc, toml)
            else:
                z.write(path, arc)
            files += 1
    print(f"wrote {out} ({files} files, Horizon {horizon[:10]})")


if __name__ == "__main__":
    main()
