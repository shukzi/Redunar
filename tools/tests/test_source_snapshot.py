"""Fixture-only coverage: no app, hardware, network, or user state."""
import importlib.util
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


if __name__ == "__main__":
    unittest.main()
