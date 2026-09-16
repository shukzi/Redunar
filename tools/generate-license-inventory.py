#!/usr/bin/env python3
"""Generate the locked third-party dependency and bundled-license inventory."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys


ROOT = Path(__file__).resolve().parent.parent
OUTPUT = ROOT / "packaging" / "DEPENDENCY-LICENSES.md"
TARGET = "x86_64-unknown-linux-gnu"
LICENSE_NAME = re.compile(r"^(?:license|copying|notice|copyright|unlicense)", re.I)


def cargo_records(label: str, manifest: Path) -> list[dict[str, object]]:
    command = [
        "cargo",
        "metadata",
        "--format-version",
        "1",
        "--locked",
        "--filter-platform",
        TARGET,
        "--manifest-path",
        str(manifest),
    ]
    metadata = json.loads(subprocess.check_output(command, cwd=ROOT, text=True))
    reachable = {node["id"] for node in metadata["resolve"]["nodes"]}
    records: list[dict[str, object]] = []
    for package in metadata["packages"]:
        if package["id"] not in reachable or not package.get("source"):
            continue
        package_root = Path(package["manifest_path"]).parent
        files = license_files(package_root, package.get("license_file"))
        records.append(
            {
                "ecosystem": "Rust",
                "context": label,
                "name": package["name"],
                "version": package["version"],
                "license": package.get("license") or "MISSING",
                "authors": ", ".join(package.get("authors") or []),
                "repository": package.get("repository") or package.get("homepage") or "",
                "source": package["source"],
                "files": files,
            }
        )
    return records


def license_files(package_root: Path, declared: str | None = None) -> list[Path]:
    candidates: set[Path] = set()
    if declared:
        declared_path = (package_root / declared).resolve()
        if declared_path.is_file() and declared_path.is_relative_to(package_root.resolve()):
            candidates.add(declared_path)
    if package_root.is_dir():
        for path in package_root.iterdir():
            if path.is_file() and LICENSE_NAME.match(path.name):
                candidates.add(path)
    return sorted(candidates, key=lambda path: path.name.lower())


def node_records() -> list[dict[str, object]]:
    ui_root = ROOT / "output" / "tauri-redunar"
    lock = json.loads((ui_root / "package-lock.json").read_text())
    records: list[dict[str, object]] = []
    for install_path, package in sorted(lock.get("packages", {}).items()):
        if not install_path:
            continue
        marker = "node_modules/"
        if marker not in install_path:
            continue
        name = install_path.rsplit(marker, 1)[1]
        records.append(
            {
                "ecosystem": "npm",
                "context": "Tauri web UI lockfile",
                "name": name,
                "version": package.get("version", "MISSING"),
                "license": package.get("license", "MISSING"),
                "authors": "",
                "repository": repository_text(package.get("repository")),
                "source": package.get("resolved", "package-lock.json"),
                # package-lock.json is the reproducible npm source of truth.
                # Runtime license texts that must ship are retained explicitly
                # under output/tauri-redunar/licenses instead of depending on
                # a developer's mutable node_modules directory.
                "files": [],
            }
        )
    return records


def repository_text(value: object) -> str:
    if isinstance(value, str):
        return value
    if isinstance(value, dict):
        return str(value.get("url") or "")
    return ""


def cell(value: object) -> str:
    return str(value or "—").replace("|", "\\|").replace("\n", " ")


def render() -> str:
    records = cargo_records("shared Rust workspace", ROOT / "Cargo.toml")
    records += cargo_records(
        "production Tauri workspace",
        ROOT / "output" / "tauri-redunar" / "src-tauri" / "Cargo.toml",
    )
    records += node_records()

    merged: dict[tuple[str, str, str, str], dict[str, object]] = {}
    for record in records:
        key = (
            str(record["ecosystem"]),
            str(record["name"]),
            str(record["version"]),
            str(record["source"]),
        )
        if key not in merged:
            merged[key] = record | {"contexts": {record["context"]}}
        else:
            merged[key]["contexts"].add(record["context"])
            known = set(merged[key]["files"])
            merged[key]["files"] = sorted(known | set(record["files"]))

    ordered = sorted(
        merged.values(),
        key=lambda item: (
            str(item["ecosystem"]).lower(),
            str(item["name"]).lower(),
            str(item["version"]),
        ),
    )
    missing_metadata = [item for item in ordered if item["license"] == "MISSING"]
    if missing_metadata:
        names = ", ".join(f'{item["name"]} {item["version"]}' for item in missing_metadata)
        raise RuntimeError(f"dependencies without license metadata: {names}")

    text_groups: dict[str, dict[str, object]] = {}
    packages_without_files: list[dict[str, object]] = []
    for item in ordered:
        files = item["files"]
        if not files:
            packages_without_files.append(item)
            continue
        for path in files:
            data = path.read_bytes()
            try:
                body = data.decode("utf-8")
            except UnicodeDecodeError:
                body = data.decode("latin-1")
            digest = hashlib.sha256(data).hexdigest()
            group = text_groups.setdefault(digest, {"body": body, "origins": []})
            group["origins"].append(
                f'{item["ecosystem"]}: {item["name"]} {item["version"]} ({path.name})'
            )

    rust_count = sum(item["ecosystem"] == "Rust" for item in ordered)
    npm_count = sum(item["ecosystem"] == "npm" for item in ordered)
    lines = [
        "# Locked dependency licenses",
        "",
        "Generated by `python3 tools/generate-license-inventory.py` from the two",
        f"locked Cargo graphs for `{TARGET}` and the Tauri `package-lock.json`.",
        "Do not edit this file by hand.",
        "",
        f"Inventory: {rust_count} unique Rust packages and {npm_count} npm lock entries.",
        "License texts below are copied verbatim from the downloaded package archives",
        "when those archives provide a standalone license/notice file.",
        "",
        "## Package inventory",
        "",
        "| Ecosystem | Package | Version | License | Build context | Authors / repository |",
        "| --- | --- | --- | --- | --- | --- |",
    ]
    for item in ordered:
        attribution = item["authors"] or item["repository"]
        lines.append(
            "| "
            + " | ".join(
                [
                    cell(item["ecosystem"]),
                    cell(item["name"]),
                    cell(item["version"]),
                    cell(item["license"]),
                    cell(", ".join(sorted(item["contexts"]))),
                    cell(attribution),
                ]
            )
            + " |"
        )

    lines += [
        "",
        "## Packages whose archive has no standalone license file",
        "",
        "Their manifest license expression is included above. Verify the upstream",
        "workspace-level notice when preparing a public binary release.",
        "",
    ]
    if packages_without_files:
        for item in packages_without_files:
            suffix = f' — {item["repository"]}' if item["repository"] else ""
            lines.append(
                f'- {item["ecosystem"]} `{item["name"]} {item["version"]}`: '
                f'`{item["license"]}`{suffix}'
            )
    else:
        lines.append("None.")

    lines += ["", "## License and notice texts", ""]
    for index, (digest, group) in enumerate(sorted(text_groups.items()), start=1):
        lines += [
            f"### Notice {index} — SHA-256 `{digest}`",
            "",
            "Packages: " + "; ".join(sorted(group["origins"])),
            "",
            "~~~~text",
            str(group["body"]).rstrip(),
            "~~~~",
            "",
        ]
    return "\n".join(lines).rstrip() + "\n"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true", help="fail if the committed inventory is stale")
    args = parser.parse_args()
    generated = render()
    if args.check:
        # Some upstream notice files contain CRLF. Compare exact bytes so
        # Python's universal-newline reader cannot report a false stale file.
        if not OUTPUT.is_file() or OUTPUT.read_bytes() != generated.encode():
            print(
                "packaging/DEPENDENCY-LICENSES.md is stale; run "
                "python3 tools/generate-license-inventory.py",
                file=sys.stderr,
            )
            return 1
        print("Locked dependency license inventory is current.")
        return 0
    OUTPUT.write_text(generated)
    print(f"Wrote {OUTPUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
