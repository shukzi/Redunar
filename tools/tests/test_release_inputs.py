"""Check release metadata agreement without builds, downloads, or installation."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

TOOLS = Path(__file__).resolve().parents[1]
VERSION = '0.1.24'


class ReleaseInputTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        (self.root / 'tools').mkdir()
        shutil.copy2(TOOLS / 'check-release-inputs.sh', self.root / 'tools')
        self.spec = self.root / 'packaging/redunar-app.spec'
        self.spec.parent.mkdir()
        self.spec.write_text(f'Name: redunar-app\nVersion: {VERSION}\n')
        self.native = self.root / 'output/tauri-redunar/src-tauri'
        self.native.mkdir(parents=True)
        (self.native / 'Cargo.toml').write_text(
            f'[package]\nname = "redunar-tauri"\nversion = "{VERSION}"\n')
        self.config = self.native / 'tauri.conf.json'
        self.config.write_text(json.dumps({'version': VERSION}))
        self.metainfo = self.root / 'packaging/com.redunar.Redunar.metainfo.xml'
        self.metainfo.write_text(
            f'<releases>\n<release version="{VERSION}"/>\n'
            '<release version="0.1.23"/>\n</releases>\n')

    def run_check(self, tag=f'v{VERSION}'):
        return subprocess.run(
            ['bash', str(self.root / 'tools/check-release-inputs.sh'), tag],
            capture_output=True, text=True, timeout=10)

    def assert_rejected(self, result):
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertNotIn('release inputs match', result.stdout)

    def test_matching_metadata(self):
        result = self.run_check()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(f'release inputs match v{VERSION}', result.stdout)

    def test_stale_tauri_config_rejected(self):
        self.config.write_text(json.dumps({'version': '0.1.23'}))
        result = self.run_check()
        self.assert_rejected(result)
        self.assertIn('tauri_config=0.1.23', result.stderr)

    def test_invalid_tauri_config_rejected(self):
        for content in ('{', '{}', '{"version": null}', '{"version": 24}'):
            with self.subTest(content=content):
                self.config.write_text(content)
                self.assert_rejected(self.run_check())

    def test_missing_tauri_config_rejected(self):
        self.config.unlink()
        self.assert_rejected(self.run_check())

    def test_other_version_mismatches_rejected(self):
        for path, content, component in (
            (self.spec, 'Version: 0.1.23\n', 'spec'),
            (self.native / 'Cargo.toml', 'version = "0.1.23"\n', 'tauri'),
            (self.metainfo, '<release version="0.1.23"/>\n', 'metainfo'),
        ):
            with self.subTest(component=component):
                original = path.read_text()
                path.write_text(content)
                result = self.run_check()
                self.assert_rejected(result)
                self.assertIn(f'{component}=0.1.23', result.stderr)
                path.write_text(original)

    def test_invalid_release_tag_rejected(self):
        for tag in ('', VERSION, 'v0.1', 'v0.1.24-rc.1'):
            with self.subTest(tag=tag):
                result = self.run_check(tag)
                self.assert_rejected(result)
                self.assertIn('release tag must use vMAJOR.MINOR.PATCH', result.stderr)


if __name__ == '__main__':
    unittest.main()
