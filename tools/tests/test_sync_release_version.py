"""Check that release-facing version metadata can be synchronized together."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


TOOLS = Path(__file__).resolve().parents[1]


class SyncReleaseVersionTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        (self.root / 'tools').mkdir()
        shutil.copy2(TOOLS / 'sync-release-version.sh', self.root / 'tools')
        (self.root / 'packaging').mkdir()
        (self.root / 'output/tauri-redunar/src-tauri').mkdir(parents=True)
        (self.root / 'packaging/redunar-app.spec').write_text(
            'Name: redunar-app\nVersion: 0.1.0\n')
        (self.root / 'output/tauri-redunar/src-tauri/Cargo.toml').write_text(
            '[package]\nname = "redunar-tauri"\nversion = "0.1.0"\n')
        (self.root / 'output/tauri-redunar/src-tauri/tauri.conf.json').write_text(
            '{\n  "version": "0.1.0",\n  "productName": "Redunar"\n}\n')
        (self.root / 'packaging/com.redunar.Redunar.metainfo.xml').write_text(
            '<releases>\n'
            '  <release version="0.1.0" date="2026-10-30"/>\n'
            '  <release version="0.0.9" date="2026-10-29"/>\n'
            '</releases>\n')

    def run_sync(self, version):
        return subprocess.run(
            ['bash', str(self.root / 'tools/sync-release-version.sh'), version],
            capture_output=True,
            text=True,
            timeout=10,
        )

    def test_updates_only_release_facing_inputs(self):
        result = self.run_sync('0.1.24')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('Version: 0.1.24',
                      (self.root / 'packaging/redunar-app.spec').read_text())
        self.assertIn('version = "0.1.24"',
                      (self.root / 'output/tauri-redunar/src-tauri/Cargo.toml').read_text())
        self.assertEqual(
            json.loads((self.root / 'output/tauri-redunar/src-tauri/tauri.conf.json').read_text())[
                'version'
            ],
            '0.1.24',
        )
        metainfo = (self.root / 'packaging/com.redunar.Redunar.metainfo.xml').read_text()
        self.assertIn('version="0.1.24"', metainfo)
        self.assertIn('version="0.0.9"', metainfo)

    def test_rejects_non_release_version(self):
        result = self.run_sync('v0.1.24')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('usage: tools/sync-release-version.sh', result.stderr)


if __name__ == '__main__':
    unittest.main()
