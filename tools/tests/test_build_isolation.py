"""Exercise build/consumer failure boundaries with fake container output."""
import fcntl
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import time
import unittest

TOOLS = Path(__file__).resolve().parents[1]
ARTIFACTS = (
    'redunar-tauri', 'redunar-steam-launch', 'redunar-hotkey-helper',
    'redunar-update-helper', 'libredunar_capture_vulkan.so', 'libredunar_capture_opengl.so',
)


class BuildIsolationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        base = Path(self.temporary.name)
        self.root = base / 'checkout'
        (self.root / 'tools/lib').mkdir(parents=True)
        for name in ('build-linux-release.sh', 'source-snapshot.py', 'prepare-release.sh'):
            shutil.copy2(TOOLS / name, self.root / 'tools' / name)
        shutil.copy2(TOOLS / 'lib/release-paths.sh', self.root / 'tools/lib/release-paths.sh')
        (self.root / '.gitignore').write_text('.redunar-build/\nnode_modules/\ntarget/\n')
        (self.root / 'output/tauri-redunar').mkdir(parents=True)
        (self.root / 'output/tauri-redunar/package.json').write_text('{}')
        subprocess.run(['git', 'init', '-q', str(self.root)], check=True)
        subprocess.run(['git', 'add', '.'], cwd=self.root, check=True)
        subprocess.run(['git', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@invalid',
                        'commit', '-qm', 'fixture'], cwd=self.root, check=True)
        (self.root / 'output/tauri-redunar/node_modules').mkdir(parents=True)
        (self.root / 'output/tauri-redunar/node_modules/fixture').write_text('dependency')
        (self.root / 'target').mkdir()
        (self.root / 'target/keep').write_text('host build')
        cargo_home = base / 'cargo'
        (cargo_home / 'registry').mkdir(parents=True)
        bin_directory = base / 'bin'
        bin_directory.mkdir()
        podman = bin_directory / 'podman'
        podman.write_text('''#!/usr/bin/env python3
import os, sys, time
from pathlib import Path
if os.environ.get('FAKE_FAIL'):
    sys.exit(9)
if sys.argv[1] == 'build':
    Path(sys.argv[sys.argv.index('--iidfile') + 1]).write_text('sha256:fixture-image')
if sys.argv[1] == 'run':
    assert 'sha256:fixture-image' in sys.argv
    Path(sys.argv[sys.argv.index('--cidfile') + 1]).write_text('fixture-container')
    if os.environ.get('FAKE_WAIT'):
        Path(os.environ['FAKE_WAIT']).write_text(str(os.getpid()))
        time.sleep(30)
    build = next(arg[:-len(':/build:Z')] for arg in sys.argv if arg.endswith(':/build:Z'))
    output = Path(build) / 'native/tauri/release'
    output.mkdir(parents=True, exist_ok=True)
    for name in ''' + repr(ARTIFACTS) + ''':
        (output / name).write_text('fake binary')
''')
        podman.chmod(0o755)
        objdump = bin_directory / 'objdump'
        objdump.write_text('#!/bin/sh\nprintf "%s\\n" "symbol (GLIBC_2.34)"\n')
        objdump.chmod(0o755)
        self.environment = dict(os.environ, PATH=f'{bin_directory}:{os.environ["PATH"]}', CARGO_HOME=str(cargo_home))
        self.environment.pop('REDUNAR_RELEASE_ROOT', None)

    def build(self, **extra):
        return subprocess.run(['bash', 'tools/build-linux-release.sh'], cwd=self.root,
                              env=dict(self.environment, **extra), capture_output=True, text=True)

    def test_success_preserves_host_cache_and_records_completed_payload(self):
        result = self.build()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.root / 'target/keep').read_text(), 'host build')
        self.assertTrue((self.root / '.redunar-build/linux/runtime.sha256').is_file())
        self.assertFalse(list((self.root / '.redunar-build').glob('source.*')))
        result = subprocess.run(['bash', '-c', 'workspace_root="$PWD"; source tools/lib/release-paths.sh; verify_release_artifacts'],
                                cwd=self.root, env=self.environment)
        self.assertEqual(result.returncode, 0)

    def test_failed_rebuild_invalidates_previous_success_and_cleans_snapshot(self):
        self.assertEqual(self.build().returncode, 0)
        self.assertNotEqual(self.build(FAKE_FAIL='1').returncode, 0)
        self.assertFalse((self.root / '.redunar-build/linux/runtime.sha256').exists())
        self.assertFalse(list((self.root / '.redunar-build').glob('source.*')))
        self.assertEqual((self.root / 'target/keep').read_text(), 'host build')

    def test_lock_refuses_second_build_without_removing_success_marker(self):
        self.assertEqual(self.build().returncode, 0)
        with (self.root / '.redunar-build/linux/build.lock').open('w') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            result = self.build()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('already running', result.stderr)
        self.assertTrue((self.root / '.redunar-build/linux/runtime.sha256').exists())

    def test_candidate_rejects_dirty_source_before_building(self):
        (self.root / 'unreviewed').write_text('change')
        result = subprocess.run(['bash', 'tools/prepare-release.sh', 'v0.1.10'],
                                cwd=self.root, env=self.environment, capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('clean, committed', result.stderr)
        self.assertFalse((self.root / '.redunar-build').exists())

    def test_candidate_rejects_invalid_tag(self):
        result = subprocess.run(['bash', 'tools/prepare-release.sh', '../main'],
                                cwd=self.root, env=self.environment, capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)
        self.assertFalse((self.root / '.redunar-build').exists())

    def test_changed_payload_is_rejected(self):
        self.assertEqual(self.build().returncode, 0)
        (self.root / '.redunar-build/linux/native/tauri/release/redunar-tauri').write_text('changed')
        result = subprocess.run(['bash', '-c', 'workspace_root="$PWD"; source tools/lib/release-paths.sh; verify_release_artifacts'],
                                cwd=self.root, env=self.environment)
        self.assertNotEqual(result.returncode, 0)

    def test_packaging_lock_refuses_in_progress_build(self):
        self.assertEqual(self.build().returncode, 0)
        with (self.root / '.redunar-build/linux/build.lock').open('w') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            result = subprocess.run(
                ['bash', '-c', 'workspace_root="$PWD"; source tools/lib/release-paths.sh; lock_release_artifacts'],
                cwd=self.root, env=self.environment, capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('updating', result.stderr)

    def test_cancellation_stops_child_and_removes_snapshot(self):
        ready = Path(self.temporary.name) / 'ready'
        process = subprocess.Popen(['bash', 'tools/build-linux-release.sh'], cwd=self.root,
                                   env=dict(self.environment, FAKE_WAIT=str(ready)),
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            deadline = time.monotonic() + 5
            while not ready.exists() and time.monotonic() < deadline:
                time.sleep(0.01)
            self.assertTrue(ready.exists(), 'fake container did not start')
            child_pid = int(ready.read_text())
            process.send_signal(signal.SIGTERM)
            process.communicate(timeout=5)
            self.assertEqual(process.returncode, 143)
            self.assertFalse(list((self.root / '.redunar-build').glob('source.*')))
            self.assertFalse((self.root / '.redunar-build/linux/runtime.sha256').exists())
            with self.assertRaises(ProcessLookupError):
                os.kill(child_pid, 0)
        finally:
            if process.poll() is None:
                process.kill()
                process.communicate()


if __name__ == '__main__':
    unittest.main()
