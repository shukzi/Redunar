"""Fixture-only coverage: no app, hardware, network, or user state."""
import importlib.util
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "source-snapshot.py"
spec = importlib.util.spec_from_file_location("source_snapshot", SCRIPT)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class SnapshotTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name) / "source"
        self.root.mkdir()
        self.git("init", "-q")
        (self.root / ".gitignore").write_text("target/\n.redunar-build/\n*.key\n")
        (self.root / "tracked").write_text("original")
        self.git("add", ".")
        self.git("-c", "user.name=Fixture", "-c", "user.email=fixture@invalid", "commit", "-qm", "fixture")

    def git(self, *arguments):
        return subprocess.check_output(["git", *arguments], cwd=self.root)

    def capture(self, name="copy"):
        destination = Path(self.temporary.name) / name
        return destination, module.snapshot(self.root, destination)

    def test_modified_new_deleted_and_ignored_files(self):
        (self.root / "tracked").write_text("modified")
        (self.root / "new").write_text("new file")
        (self.root / "private.key").write_text("fixture secret")
        (self.root / "target").mkdir()
        (self.root / "target/stale").write_text("old artifact")
        destination, manifest = self.capture()
        self.assertEqual((destination / "tracked").read_text(), "modified")
        self.assertTrue((destination / "new").exists())
        self.assertFalse((destination / "target").exists())
        self.assertFalse((destination / "private.key").exists())
        self.assertFalse((destination / ".git").exists())
        self.assertEqual(len(manifest["source_sha256"]), 64)
        (self.root / "tracked").unlink()
        other, changed = self.capture("other")
        self.assertFalse((other / "tracked").exists())
        self.assertNotEqual(manifest["source_sha256"], changed["source_sha256"])

    def test_refuses_source_symlink(self):
        (self.root / "escape").symlink_to("/etc/passwd")
        with self.assertRaisesRegex(ValueError, "symlinks"):
            self.capture()

    def test_refuses_nonempty_destination(self):
        self.capture()
        with self.assertRaisesRegex(ValueError, "empty"):
            self.capture()

    def test_two_snapshots_do_not_share_mutable_files(self):
        first, _ = self.capture()
        second, _ = self.capture("second")
        (first / "tracked").write_text("changed in build")
        self.assertEqual((second / "tracked").read_text(), "original")
        self.assertEqual((self.root / "tracked").read_text(), "original")

    def write_manifest(self, manifest):
        path = Path(self.temporary.name) / "source-manifest.json"
        path.write_text(json.dumps(manifest))
        return path

    def test_verifies_exported_files_and_executable_modes(self):
        destination, manifest = self.capture()
        path = self.write_manifest(manifest)
        self.assertEqual(module.verify_snapshot(destination, path), manifest)
        (destination / "tracked").chmod(0o755)
        with self.assertRaisesRegex(ValueError, "differs"):
            module.verify_snapshot(destination, path)

    def test_rejects_missing_inputs_and_changed_manifest(self):
        destination, manifest = self.capture()
        path = self.write_manifest(manifest)
        (destination / "tracked").unlink()
        with self.assertRaisesRegex(ValueError, "regular file"):
            module.verify_snapshot(destination, path)
        manifest["files"]["tracked"]["sha256"] = "0" * 64
        self.write_manifest(manifest)
        with self.assertRaisesRegex(ValueError, "manifest hash"):
            module.verify_snapshot(destination, path)

    def test_rejects_manifest_path_escape_and_symlink_parent(self):
        destination, manifest = self.capture()
        for name in ("../tracked", "/tracked", "folder/../tracked"):
            with self.subTest(name=name):
                altered = dict(manifest, files={name: manifest["files"]["tracked"]})
                altered["source_sha256"] = hashlib.sha256(
                    json.dumps(altered["files"], sort_keys=True).encode()).hexdigest()
                with self.assertRaisesRegex(ValueError, "unsafe path"):
                    module.verify_snapshot(destination, self.write_manifest(altered))
        (destination / "linked").symlink_to(self.root, target_is_directory=True)
        manifest["files"] = {"linked/tracked": manifest["files"]["tracked"]}
        manifest["source_sha256"] = hashlib.sha256(
            json.dumps(manifest["files"], sort_keys=True).encode()).hexdigest()
        with self.assertRaisesRegex(ValueError, "symlinks"):
            module.verify_snapshot(destination, self.write_manifest(manifest))


if __name__ == "__main__":
    unittest.main()
