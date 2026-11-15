#!/usr/bin/env python3
"""Copy Git-visible working files, excluding ignored caches and private state."""
import hashlib
import json
from pathlib import Path, PurePosixPath
import shutil
import subprocess
import sys


def verify_snapshot(root, manifest_path):
    """Verify the declared inputs of an exported source tree without Git."""
    root = root.resolve()
    manifest = json.loads(manifest_path.read_text())
    commit = manifest.get("commit")
    records = manifest.get("files")
    if (not isinstance(commit, str) or len(commit) not in (40, 64)
            or any(character not in "0123456789abcdef" for character in commit)
            or not isinstance(records, dict) or not records):
        raise ValueError("invalid source snapshot manifest")
    digest = hashlib.sha256(json.dumps(records, sort_keys=True).encode()).hexdigest()
    if digest != manifest.get("source_sha256"):
        raise ValueError("source snapshot manifest hash does not match its inputs")
    for name, expected in records.items():
        relative = PurePosixPath(name)
        if (not name or relative.is_absolute() or ".." in relative.parts
                or str(relative) != name or "\0" in name):
            raise ValueError(f"source snapshot refuses unsafe path: {name}")
        source = root/name
        current = source
        while current != root:
            if current.is_symlink():
                raise ValueError(f"source snapshot refuses symlinks: {name}")
            current = current.parent
        if not source.is_file():
            raise ValueError(f"source snapshot requires a regular file: {name}")
        if (not isinstance(expected, dict) or set(expected) != {"sha256", "executable"}
                or not isinstance(expected["sha256"], str)
                or type(expected["executable"]) is not bool):
            raise ValueError(f"invalid source snapshot record: {name}")
        actual = {
            "sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
            "executable": bool(source.stat().st_mode & 0o111),
        }
        if actual != expected:
            raise ValueError(f"source differs from snapshot manifest: {name}")
    return {"commit": commit, "files": records, "source_sha256": digest}


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
