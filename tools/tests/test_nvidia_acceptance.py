"""Check runner opt-in and state isolation without opening a graphics device."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

TOOLS = Path(__file__).resolve().parents[1]


class NvidiaAcceptanceTests(unittest.TestCase):
    def test_beta_flag_uses_private_runtime_and_offline_checkout_target(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / 'tools/lib').mkdir(parents=True)
            runner = root / 'tools/run-tauri-vulkan-replay-acceptance.sh'
            shutil.copy2(TOOLS / runner.name, runner)
            shutil.copy2(TOOLS / 'lib/release-paths.sh', root / 'tools/lib/release-paths.sh')
            release = root / 'release'
            release.mkdir()
            (release / 'libredunar_capture_vulkan.so').write_bytes(b'fixture')
            binary = root / 'bin'
            binary.mkdir()
            report = root / 'report.json'
            cargo = binary / 'cargo'
            cargo.write_text('''#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
keys = ('HOME', 'XDG_STATE_HOME', 'XDG_RUNTIME_DIR', 'XDG_CONFIG_HOME',
        'REDUNAR_TAURI_TEST_STATE', 'REDUNAR_TAURI_BETA_ACCESS')
Path(os.environ['FIXTURE_REPORT']).write_text(json.dumps({
    'env': {key: os.environ[key] for key in keys}, 'args': sys.argv[1:]}))
state = Path(os.environ['REDUNAR_TAURI_TEST_STATE'])
state.mkdir(parents=True)
(state / 'diagnostics.log').write_text('safe fixture reason=missing_extension')
sys.exit(int(os.environ.get('FIXTURE_EXIT', '0')))
''')
            cargo.chmod(0o755)
            environment = dict(os.environ, PATH=str(binary) + os.pathsep + os.environ['PATH'],
                               REDUNAR_RELEASE_ROOT=str(release), FIXTURE_REPORT=str(report))
            for arguments, expected in [([], '0'), (['--nvidia-beta'], '1')]:
                subprocess.run(['bash', str(runner), *arguments], env=environment,
                               check=True, capture_output=True, timeout=10)
                result = json.loads(report.read_text())
                values = result['env']
                self.assertEqual(values['REDUNAR_TAURI_BETA_ACCESS'], expected)
                self.assertEqual(values['REDUNAR_TAURI_TEST_STATE'], values['XDG_STATE_HOME'] + '/redunar')
                self.assertNotEqual(values['HOME'], os.environ['HOME'])
                self.assertNotEqual(values['XDG_RUNTIME_DIR'], os.environ.get('XDG_RUNTIME_DIR'))
                self.assertFalse(Path(values['HOME']).exists(), 'fixture is cleaned')
                self.assertIn('--locked', result['args'])
                self.assertIn('--offline', result['args'])
                self.assertIn(str(root / '.redunar-build/native/tauri'), result['args'])
            failed = subprocess.run(['bash', str(runner), '--nvidia-beta'],
                                    env=dict(environment, FIXTURE_EXIT='7'),
                                    capture_output=True, timeout=10)
            self.assertEqual(failed.returncode, 7)
            reports = list((root / '.redunar-build/reports').glob('*/diagnostics.log'))
            self.assertEqual(len(reports), 3)
            self.assertTrue(all('reason=missing_extension' in path.read_text() for path in reports))
            rejected = subprocess.run(['bash', str(runner), '--unknown'], env=environment,
                                      capture_output=True, timeout=10)
            self.assertEqual(rejected.returncode, 2)
