#!/usr/bin/env python3
"""Copy Git-visible working files, excluding ignored caches and private state."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys


def snapshot(root, destination=None):
    root = root.resolve()
    if destination is not None and destination.exists() and any(destination.iterdir()):
        raise ValueError("snapshot destination must be empty")
    if destination is not None:
        destination.mkdir(parents=True, exist_ok=True)
    names = subprocess.check_output(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"], cwd=root
    ).decode().split("\0")
    records = {}
    for name in sorted(set(filter(None, names))):
        source = root / name
        if not source.exists() and not source.is_symlink():
            continue  # A tracked deletion belongs to the current working tree.
        if source.is_symlink():
            raise ValueError(f"source snapshot refuses symlinks: {name}")
        if not source.is_file():
            raise ValueError(f"source snapshot requires a regular file: {name}")
        target = source
        if destination is not None:
            target = destination / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, target)
        records[name] = {
            "sha256": hashlib.sha256(target.read_bytes()).hexdigest(),
            "executable": bool(target.stat().st_mode & 0o111),
        }
    return {
        "commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root).decode().strip(),
        "files": records,
        "source_sha256": hashlib.sha256(json.dumps(records, sort_keys=True).encode()).hexdigest(),
    }


if __name__ == "__main__":
    if sys.argv[1] == "--manifest-only":
        source_root, manifest = map(Path, sys.argv[2:])
        output = None
    else:
        source_root, output, manifest = map(Path, sys.argv[1:])
    result = snapshot(source_root, output)
    manifest.write_text(json.dumps(result, indent=2) + "\n")
