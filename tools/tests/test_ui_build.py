"""Exercise frontend provenance in checkouts and exported source trees."""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

TOOLS = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('ui_source_snapshot', TOOLS/'source-snapshot.py')
snapshot_module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(snapshot_module)


class UiBuildTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='redunar-ui-build-')
        self.addCleanup(temporary.cleanup)
        self.base = Path(temporary.name)
        self.root = self.base/'checkout'
        (self.root/'tools').mkdir(parents=True)
        for name in ('build-ui.py', 'source-snapshot.py'):
            shutil.copy2(TOOLS/name, self.root/'tools'/name)
        (self.root/'.gitignore').write_text('.redunar-build/\n**/node_modules/\n**/dist/\n')
        (self.root/'input.js').write_text('original source')
        subprocess.run(['git', 'init', '-q', str(self.root)], check=True)
        subprocess.run(['git', 'add', '.'], cwd=self.root, check=True)
        subprocess.run(['git', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@invalid',
                        'commit', '-qm', 'fixture'], cwd=self.root, check=True)
        self.dependencies = self.root/'output/tauri-redunar/node_modules'
        (self.dependencies/'.bin').mkdir(parents=True)
        vite = self.dependencies/'.bin/vite'
        vite.write_text(f'#!{sys.executable}\n' + '''import os
from pathlib import Path
native = Path.cwd()
(native/'node_modules/vite-ran').touch()
if os.environ.get('MUTATE_SOURCE'):
    (native.parents[1]/'input.js').write_text('changed during build')
output = native/'ui/dist'
output.mkdir(parents=True)
(output/'main.js').write_text((native.parents[1]/'input.js').read_text())
''')
        vite.chmod(0o755)

    def export(self):
        root = self.base/'export'
        manifest = snapshot_module.snapshot(self.root, root)
        shutil.copytree(self.dependencies, root/'output/tauri-redunar/node_modules')
        path = self.base/'source-manifest.json'
        path.write_text(json.dumps(manifest))
        return root, path, manifest

    def build(self, root, manifest=None, **extra):
        environment = dict(os.environ, **extra)
        environment.pop('REDUNAR_SOURCE_MANIFEST', None)
        if manifest is not None:
            environment['REDUNAR_SOURCE_MANIFEST'] = str(manifest)
        return subprocess.run([sys.executable, str(root/'tools/build-ui.py')], cwd=root,
                              env=environment, capture_output=True, text=True)

    def test_checkout_keeps_git_based_source_and_asset_evidence(self):
        result = self.build(self.root)
        self.assertEqual(result.returncode, 0, result.stderr)
        evidence = json.loads((self.root/'.redunar-build/ui-build.json').read_text())
        self.assertEqual(evidence['source_sha256'], snapshot_module.snapshot(self.root)['source_sha256'])
        self.assertIn('main.js', evidence['assets'])

    def test_exported_build_uses_verified_manifest_without_git_metadata(self):
        root, path, manifest = self.export()
        self.assertFalse((root/'.git').exists())
        self.assertNotEqual(subprocess.run(['git', 'rev-parse', '--show-toplevel'], cwd=root,
                                          capture_output=True).returncode, 0)
        result = self.build(root, path)
        self.assertEqual(result.returncode, 0, result.stderr)
        evidence = json.loads((root/'.redunar-build/ui-build.json').read_text())
        self.assertEqual(evidence['commit'], manifest['commit'])
        self.assertEqual(evidence['source_sha256'], manifest['source_sha256'])
        self.assertEqual((root/'output/tauri-redunar/ui/dist/main.js').read_text(), 'original source')

    def test_export_refuses_changed_input_before_running_the_bundler(self):
        root, path, _ = self.export()
        (root/'input.js').write_text('changed before build')
        result = self.build(root, path)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((root/'output/tauri-redunar/node_modules/vite-ran').exists())
        self.assertFalse((root/'.redunar-build/ui-build.json').exists())

    def test_export_refuses_source_changes_during_build(self):
        root, path, _ = self.export()
        result = self.build(root, path, MUTATE_SOURCE='1')
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue((root/'output/tauri-redunar/node_modules/vite-ran').exists())
        self.assertFalse((root/'.redunar-build/ui-build.json').exists())

    def test_export_does_not_silently_build_without_provenance(self):
        root, _, _ = self.export()
        result = self.build(root)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((root/'output/tauri-redunar/node_modules/vite-ran').exists())


if __name__ == '__main__':
    unittest.main()
