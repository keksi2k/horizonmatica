#!/usr/bin/env python3
"""Update Horizonmatica-Upload to the newest Horizon Nightly, rebuild, package.

Usage: python update_for_newest.py [--skip-build] [--release]
- latest version: horizon-launcher versions --json (first entry)
- full commit: GitHub nightly-source/<tag> ref
- installs it, re-pins horizon-mixin.toml, native-build --update, packages .hnmod
Idempotent, resumable: steps already done are skipped/fast.
"""
import argparse, json, pathlib, re, subprocess, sys, urllib.request

LAUNCHER = r"C:\Users\Administrator\AppData\Local\Programs\Horizon Launcher\horizon-launcher.exe"
REPO = pathlib.Path(__file__).resolve().parent.parent.parent  # horizonmatica/ -> horizon-mcp/
ROOT = pathlib.Path(__file__).resolve().parents[2] if False else pathlib.Path(__file__).resolve().parent
# script lives in horizonmatica/ (fork root), mixins under mixins/
MIXINS = ROOT / "mixins" / "horizonmatica"
TOML = MIXINS / "horizon-mixin.toml"

def run(cmd, **kw):
    print("+", " ".join(cmd)); return subprocess.run(cmd, check=True, **kw)

def main():
    ap = argparse.ArgumentParser(); ap.add_argument("--skip-build", action="store_true"); ap.add_argument("--release", action="store_true"); a = ap.parse_args()
    data = subprocess.check_output([LAUNCHER, "versions", "--json"], text=True)
    newest = json.loads(data)[0]; tag = newest["tag"]
    print("newest:", tag, "installed:", newest["installed"])
    with urllib.request.urlopen(f"https://api.github.com/repos/horizon-rust/horizon/git/refs/tags/nightly-source/{tag}") as r:
        full = json.load(r)["object"]["sha"]
    print("full commit:", full)
    if not newest["installed"]:
        run([LAUNCHER, "install", tag])
    toml = TOML.read_text(encoding="utf-8")
    old = re.search(r'horizon_revision\s*=\s*"([0-9a-f]{40})"', toml).group(1)
    if old != full:
        TOML.write_text(toml.replace(old, full), encoding="utf-8"); print(f"repin {old[:7]} -> {full[:7]}")
    else:
        print("already pinned:", full[:7])
    if a.skip_build: return
    run([LAUNCHER, "native-build", str(MIXINS), "--instance", "horizon", "--trust-native-code", "--profile", "dev", "--update"])
    run([sys.executable, str(ROOT / "tools" / "package.py"), "--horizon", full])
    if a.release:
        run(["git", "-C", str(ROOT), "add", "mixins/horizonmatica/horizon-mixin.toml"])
        run(["git", "-C", str(ROOT), "commit", "-m", f"Pin to {tag} {full[:7]} (newest, follows update)"])
        run(["git", "-C", str(ROOT), "push", "origin", "main"])
        hnmod = next((ROOT / "dist").glob("*.hnmod"))
        run(["gh", "release", "create", f"v-upload-{full[:7]}", "-R", "NotDiscxrd23/horizonmatica", "--target", "main", "--title", f"Upload for {tag}", "--notes", f"Built for Horizon {full}.", str(hnmod)])

if __name__ == "__main__":
    main()
