"""Test suite discovery, build identity, prerequisites and bounded child cleanup."""
from contextlib import redirect_stdout
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('ui_runner', ROOT/'tools/test-ui.py')
RUNNER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUNNER)


class UiRunnerTests(unittest.TestCase):
    def test_every_webview_suite_has_exactly_one_group(self):
        listed = [name+'-webview.py' for names in RUNNER.GROUPS.values() for name in names]
        self.assertEqual(len(listed), len(set(listed)))
        self.assertEqual(set(listed), {path.name for path in RUNNER.TESTS.glob('*-webview.py')})

    def test_build_identity_rejects_changed_source_and_tampered_assets(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            assets = root/'output/tauri-redunar/ui/dist'
            assets.mkdir(parents=True)
            (assets/'index.html').write_text('built fixture')
            report = root/'.redunar-build/ui-build.json'
            report.parent.mkdir()
            snapshot = {'source_sha256': 'source'}
            with patch.object(RUNNER, 'ROOT', root):
                self.assertIn('missing', RUNNER.build_problem(snapshot))
                report.write_text(json.dumps({'source_sha256': 'source', 'assets': RUNNER.asset_hashes(assets)}))
                self.assertIsNone(RUNNER.build_problem(snapshot))
                self.assertIn('Source differs', RUNNER.build_problem({'source_sha256': 'changed'}))
                (assets/'index.html').write_text('stale replacement')
                self.assertIn('Served assets differ', RUNNER.build_problem(snapshot))

    def test_missing_pointer_display_is_blocked_without_using_the_host_display(self):
        with patch.object(RUNNER.shutil, 'which', return_value=None), \
                patch.object(RUNNER.subprocess, 'run', return_value=SimpleNamespace(returncode=0)):
            self.assertIn('Xvfb is unavailable', RUNNER.prerequisites('pointer'))

    def test_timed_out_child_is_stopped(self):
        with tempfile.TemporaryFile(mode='w+') as log:
            started = time.monotonic()
            result = RUNNER.run_child([sys.executable, '-c', 'import time; time.sleep(30)'], {}, log, .05)
            self.assertEqual(result['status'], 'failed')
            self.assertIn('deadline', result['reason'])
            self.assertLess(time.monotonic()-started, 3)

    def test_blocked_preflight_never_starts_a_suite(self):
        args = SimpleNamespace(list=False, group=['media'], suite=[], preflight=True, timeout=1)
        with tempfile.TemporaryDirectory() as temporary, \
                patch.object(RUNNER, 'ROOT', Path(temporary)), \
                patch.object(RUNNER, 'arguments', return_value=args), \
                patch.object(RUNNER, 'source_snapshot', return_value={'source_sha256': 'source'}), \
                patch.object(RUNNER, 'build_problem', return_value='Build missing'), \
                patch.object(RUNNER, 'prerequisites', return_value=[]), \
                patch.object(RUNNER, 'run_suite') as run, redirect_stdout(io.StringIO()) as output:
            self.assertEqual(RUNNER.main(), 2)
            self.assertEqual(json.loads(output.getvalue())['build_problem'], 'Build missing')
            run.assert_not_called()


if __name__ == '__main__':
    unittest.main()
