"""Guard the production GTK3 backport and known security release inputs."""

import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import tomllib
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
TAURI = ROOT / "output/tauri-redunar/src-tauri"


class DependencySecurityTests(unittest.TestCase):
    def test_glib_override_replaces_the_affected_registry_copy(self):
        manifest = tomllib.loads((TAURI / "Cargo.toml").read_text())
        override = manifest["patch"]["crates-io"]["glib"]["path"]
        self.assertEqual((TAURI / override).resolve(), ROOT / "vendor/glib")
        lock = tomllib.loads((TAURI / "Cargo.lock").read_text())
        copies = [p for p in lock["package"] if p["name"] == "glib"]
        self.assertEqual(len(copies), 1)
        self.assertEqual(copies[0]["version"], "0.18.5")
        self.assertNotIn("source", copies[0])

    def test_vendored_glib_matches_published_source_except_iterator_fix(self):
        vendor = ROOT / "vendor/glib"
        hashes = json.loads((vendor / "upstream-files.sha256.json").read_text())
        extras = {"REDUNAR-PATCH.md", "upstream-files.sha256.json"}
        actual = {
            str(p.relative_to(vendor))
            for p in vendor.rglob("*")
            if p.is_file() and "__pycache__" not in p.parts
        }
        self.assertEqual(actual, set(hashes) | extras)
        for name, expected in hashes.items():
            data = (vendor / name).read_bytes()
            if name == "src/variant_iter.rs":
                self.assertEqual(data.count(b"let mut p: *mut libc::c_char"), 1)
                self.assertEqual(data.count(b"                &mut p,\n"), 1)
                data = data.replace(b"let mut p: *mut libc::c_char", b"let p: *mut libc::c_char")
                data = data.replace(b"                &mut p,\n", b"                &p,\n")
            with self.subTest(file=name):
                self.assertEqual(hashlib.sha256(data).hexdigest(), expected)

    def test_tauri_and_tokio_locks_exclude_known_affected_releases(self):
        lock = tomllib.loads((TAURI / "Cargo.lock").read_text())
        versions = {p["name"]: p["version"] for p in lock["package"]}
        numeric = lambda value: tuple(int(part) for part in value.split("."))
        self.assertGreaterEqual(numeric(versions["tauri"]), (2, 12, 5))
        self.assertGreaterEqual(numeric(versions["tokio"]), (1, 53, 3))
        self.assertNotEqual(versions["tokio"], "1.54.0")

    def test_frontend_lock_excludes_affected_tooling(self):
        packages = json.loads((TAURI.parent / "package-lock.json").read_text())["packages"]
        self.assertEqual(packages["node_modules/vite"]["version"], "6.4.5")
        self.assertEqual(packages["node_modules/source-map-js"]["version"], "1.2.2")

    def test_license_inventory_retains_vendored_third_party_notices(self):
        spec = importlib.util.spec_from_file_location(
            "license_inventory", ROOT / "tools/generate-license-inventory.py"
        )
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        with tempfile.TemporaryDirectory() as temporary:
            module.ROOT = Path(temporary)
            vendor = module.ROOT / "vendor/glib"
            vendor.mkdir(parents=True)
            (vendor / "LICENSE").write_text("fixture MIT notice")
            packages = [
                {"id": "vendored", "name": "glib", "version": "0.18.5",
                 "source": None, "manifest_path": str(vendor / "Cargo.toml"),
                 "license": "MIT"},
                {"id": "owned", "name": "redunar-core", "version": "0.1.18",
                 "source": None, "manifest_path": str(module.ROOT / "Cargo.toml")},
            ]
            metadata = {"packages": packages, "resolve": {"nodes": [
                {"id": "vendored"}, {"id": "owned"}
            ]}}
            with patch.object(module.subprocess, "check_output", return_value=json.dumps(metadata)):
                records = module.cargo_records("fixture", module.ROOT / "Cargo.toml")
            self.assertEqual(len(records), 1)
            self.assertEqual(records[0]["name"], "glib")
            self.assertEqual(records[0]["source"], "vendored: vendor/glib")
            self.assertEqual(records[0]["files"], [vendor / "LICENSE"])


if __name__ == "__main__":
    unittest.main()
