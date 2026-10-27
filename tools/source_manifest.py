#!/usr/bin/env python3
"""Write or compare a source manifest: byte hashes, file modes and command environment.

    python3 tools/source_manifest.py --repo . --repo ../abi --repo ../wdbx --out manifest.json
    python3 tools/source_manifest.py --compare a.json b.json

Files are `git ls-files -co --exclude-standard` (tracked plus untracked, not ignored).
Only allowlisted, non-credential environment names are recorded.
"""
import argparse
import hashlib
import json
import os
import platform
import stat
import subprocess
import sys
from pathlib import Path

ENV_EXACT = ("CARGO_TARGET_DIR", "RUSTUP_TOOLCHAIN")
ENV_PREFIX = "ABBEY_"
CREDENTIAL_MARKERS = ("TOKEN", "KEY", "SECRET", "BEARER", "PASSWORD")


def sha256_file(path):
    digest = hashlib.sha256()
    if os.path.islink(path):
        digest.update(os.readlink(path).encode("utf-8", "surrogateescape"))
        return digest.hexdigest()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def capture_env(environ):
    recorded = {}
    for name, value in environ.items():
        upper = name.upper()
        if any(marker in upper for marker in CREDENTIAL_MARKERS):
            continue
        if name in ENV_EXACT or name.startswith(ENV_PREFIX):
            recorded[name] = value
    return dict(sorted(recorded.items()))


def run_text(argv, cwd=None):
    try:
        result = subprocess.run(argv, cwd=cwd, capture_output=True, text=True, check=True)
    except (OSError, subprocess.CalledProcessError):
        return "unavailable"
    return result.stdout.strip()


def repo_entry(repo):
    repo = Path(repo)
    listing = subprocess.run(
        ["git", "-C", str(repo), "ls-files", "-z", "-co", "--exclude-standard"],
        capture_output=True,
        check=True,
    ).stdout
    files = {}
    for raw in sorted(set(filter(None, listing.split(b"\0")))):
        rel = raw.decode("utf-8", "surrogateescape")
        path = repo / rel
        try:
            mode = os.lstat(path).st_mode
        except FileNotFoundError:
            continue  # tracked but deleted in the working tree
        if stat.S_ISDIR(mode):
            continue  # gitlink/submodule
        files[rel] = {"mode": mode, "sha256": sha256_file(path)}
    return {"head": run_text(["git", "-C", str(repo), "rev-parse", "HEAD"]), "files": files}


def build_manifest(repos, environ=None):
    environ = os.environ if environ is None else environ
    manifest = {}
    for repo in repos:
        name = Path(repo).resolve().name
        if name in manifest or name == "environment":
            raise SystemExit(f"duplicate or reserved repo name: {name}")
        manifest[name] = repo_entry(repo)
    manifest["environment"] = {
        "rustc": run_text(["rustc", "--version"]),
        "cargo": run_text(["cargo", "--version"]),
        "python": platform.python_version(),
        "env": capture_env(environ),
    }
    return manifest


def compare_manifests(a, b):
    """Return sorted differing paths ("repo/path"; "environment/<key>" for environment)."""
    differing = set()
    for name in set(a) | set(b):
        left, right = a.get(name), b.get(name)
        if name == "environment":
            left, right = left or {}, right or {}
            for key in set(left) | set(right):
                if left.get(key) != right.get(key):
                    differing.add(f"environment/{key}")
            continue
        if left is None or right is None:
            differing.add(name)
            continue
        if left.get("head") != right.get("head"):
            differing.add(f"{name}/(head)")
        lf, rf = left.get("files", {}), right.get("files", {})
        for rel in set(lf) | set(rf):
            if lf.get(rel) != rf.get(rel):
                differing.add(f"{name}/{rel}")
    return sorted(differing)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--repo", action="append", default=[])
    parser.add_argument("--out")
    parser.add_argument("--compare", nargs=2, metavar=("A", "B"))
    args = parser.parse_args(argv)
    if args.compare:
        loaded = []
        for path in args.compare:
            with open(path, encoding="utf-8") as handle:
                loaded.append(json.load(handle))
        differences = compare_manifests(*loaded)
        for path in differences:
            print(path)
        return 1 if differences else 0
    if not args.repo or not args.out:
        parser.error("--repo (one or more) and --out are required unless --compare is used")
    manifest = build_manifest(args.repo)
    with open(args.out, "w", encoding="utf-8") as handle:
        json.dump(manifest, handle, indent=2, sort_keys=True)
        handle.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
