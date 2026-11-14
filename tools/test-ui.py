#!/usr/bin/env python3
"""Run synthetic UI suites on a private display, using verified production assets."""
import argparse
from contextlib import contextmanager
from datetime import datetime, timezone
import fcntl
import hashlib
import json
import os
from pathlib import Path
import runpy
import shutil
import signal
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
TESTS = ROOT/'output/tauri-redunar/tests'
GROUPS = {
    'generic': ('capture-diagnostics', 'clip-rail-layout', 'clip-scrolling', 'history-motion', 'interactions',
                'layout', 'library-tabs', 'motion', 'overlay-metrics', 'overview-scroll', 'performance',
                'refinements', 'replay-inventory', 'startup-inventory', 'thumbnail', 'toasts', 'trash-bin', 'workspace'),
    'media': ('audio-controls', 'player-updates', 'replay'),
    'pointer': ('overview-wheel', 'regular-usage', 'ui-polish'),
}
PROBES = ('tools/run-tauri-vulkan-session-acceptance.sh', 'tools/run-tauri-vulkan-replay-acceptance.sh',
          'tools/run-opengl-overlay-acceptance.sh', 'tools/run-opengl-replay-acceptance.sh')


def source_snapshot():
    return runpy.run_path(str(ROOT/'tools/source-snapshot.py'))['snapshot'](ROOT)


def asset_hashes(directory):
    return {str(path.relative_to(directory)): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in sorted(directory.rglob('*')) if path.is_file()}


def build_problem(snapshot):
    path = ROOT/'.redunar-build/ui-build.json'
    if not path.is_file():
        return 'Build evidence is missing. Run npm --prefix output/tauri-redunar run build.'
    try:
        build = json.loads(path.read_text())
        if build['source_sha256'] != snapshot['source_sha256']:
            return 'Source differs from the UI build. Rebuild the production frontend.'
        assets = asset_hashes(ROOT/'output/tauri-redunar/ui/dist')
        if not assets or build['assets'] != assets:
            return 'Served assets differ from the verified UI build. Rebuild the production frontend.'
    except (OSError, ValueError, KeyError) as error:
        return 'Invalid UI build evidence: '+str(error)
    return None


def prerequisites(group):
    missing = []
    display = 'Xvfb' if group == 'pointer' else 'broadwayd'
    for name in [display, *(['ffmpeg', 'ffprobe'] if group in ('media', 'pointer') else [])]:
        if shutil.which(name) is None:
            missing.append(name+' is unavailable')
    result = subprocess.run([sys.executable, '-c',
                             "import gi; gi.require_version('Gtk','3.0'); gi.require_version('WebKit2','4.1'); from gi.repository import Gtk, WebKit2"],
                            capture_output=True, text=True, timeout=10)
    if result.returncode:
        missing.append('Python GI, GTK3 and WebKitGTK 4.1 are required: '+result.stderr.strip())
    return missing


def stop_process(process):
    """Release only this runner's process group, including fixture descendants."""
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    try:
        process.wait(timeout=3)
    except subprocess.TimeoutExpired:
        pass
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    process.wait(timeout=3)


@contextmanager
def private_display(group, log):
    with tempfile.TemporaryDirectory(prefix='rd-webview-') as temporary:
        private = Path(temporary)
        private.chmod(0o700)
        environment = dict(os.environ, XDG_RUNTIME_DIR=temporary, XDG_CONFIG_HOME=str(private/'config'),
                           XDG_CACHE_HOME=str(private/'cache'), XDG_STATE_HOME=str(private/'state'),
                           DBUS_SESSION_BUS_ADDRESS='unix:path='+str(private/'no-bus'), XAUTHORITY=str(private/'Xauthority'),
                           GSETTINGS_BACKEND='memory', GIO_USE_VFS='local', GTK_USE_PORTAL='0', REDUNAR_PRIVATE_DESKTOP_FIXTURE='1')
        if group == 'pointer':
            # -displayfd chooses a free display. No host display or input is used.
            read_fd, write_fd = os.pipe()
            daemon = subprocess.Popen(['Xvfb', '-displayfd', str(write_fd), '-screen', '0', '2560x1600x24', '-nolisten', 'tcp'],
                                      env=environment, stdout=log, stderr=log, pass_fds=(write_fd,), start_new_session=True)
            os.close(write_fd)
            import select
            if not select.select([read_fd], [], [], 5)[0]:
                os.close(read_fd)
                stop_process(daemon)
                raise TimeoutError('Private X11 display did not start')
            display = os.read(read_fd, 32).decode().strip()
            os.close(read_fd)
            environment.update(GDK_BACKEND='x11', DISPLAY=':'+display)
        else:
            environment.update(GDK_BACKEND='broadway', BROADWAY_DISPLAY=':1')
            socket = private/'http.sock'
            daemon = subprocess.Popen(['broadwayd', '--unixsocket='+str(socket), ':1'], env=environment,
                                      stdout=log, stderr=log, start_new_session=True)
            deadline = time.monotonic()+5
            while not socket.exists():
                if daemon.poll() is not None or time.monotonic() >= deadline:
                    stop_process(daemon)
                    raise RuntimeError('Private Broadway display did not start')
                time.sleep(.05)
        try:
            yield environment
        finally:
            stop_process(daemon)


def run_child(command, environment, log, timeout):
    process = subprocess.Popen(command, cwd=ROOT, env=environment, stdout=log, stderr=log, start_new_session=True)
    try:
        result = process.wait(timeout=timeout)
        return {'status': 'passed' if result == 0 else 'failed', 'exit_code': result}
    except subprocess.TimeoutExpired:
        return {'status': 'failed', 'reason': 'Suite deadline exceeded', 'timeout_seconds': timeout}
    finally:
        stop_process(process)


def run_suite(name, group, directory, timeout):
    directory.mkdir(parents=True)
    with (directory/'output.log').open('w') as log:
        with private_display(group, log) as environment:
            environment['REDUNAR_UI_ARTIFACTS'] = str(directory)
            result = run_child([sys.executable, str(TESTS/(name+'-webview.py'))], environment, log, timeout)
    result['artifacts'] = str(directory.relative_to(ROOT))
    evidence = directory/'fixture-result.json'
    if evidence.exists():
        details = json.loads(evidence.read_text())
        result['checks'] = len(details['checks'])
        if details['status'] != 'passed':
            result['status'] = 'failed'
            result['reason'] = 'Fixture reported failure'
    return result


def arguments():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--list', action='store_true', help='List suites, prerequisites and separate native probes without launching them')
    parser.add_argument('--group', action='append', choices=tuple(GROUPS), default=[])
    parser.add_argument('--suite', action='append', choices=tuple(name for names in GROUPS.values() for name in names), default=[])
    parser.add_argument('--preflight', action='store_true', help='Report prerequisite/build problems without starting a display or suite')
    parser.add_argument('--timeout', type=int, default=300, help='Maximum seconds per suite (default: 300)')
    return parser.parse_args()


def main():
    args = arguments()
    if args.list:
        for group, names in GROUPS.items():
            print(group+': '+', '.join(names))
        print('All groups require Python GI, GTK3, WebKitGTK 4.1 and a verified production UI build.')
        print('generic/media use broadwayd. media also requires FFmpeg/FFprobe and WebKit video codecs.')
        print('pointer requires Xvfb, FFmpeg/FFprobe and X11 input libraries. Host displays are never used.')
        print('Native/hardware probes remain separate and need explicit runtime authorization: '+', '.join(PROBES))
        return 0
    if args.timeout <= 0:
        raise SystemExit('--timeout must be positive')
    groups = args.group or ([] if args.suite else ['generic'])
    selected = {name for group in groups for name in GROUPS[group]} | set(args.suite)
    build_root = ROOT/'.redunar-build'
    build_root.mkdir(exist_ok=True)
    with (build_root/'validation.lock').open('a') as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise SystemExit('Validation is already running in this checkout.')
        before = source_snapshot()
        problem = build_problem(before)
        required = {group: prerequisites(group) for group, names in GROUPS.items() if set(names) & selected}
        if args.preflight:
            print(json.dumps({'build_problem': problem, 'prerequisites': required}, indent=2))
            return 2 if problem or any(required.values()) else 0
        reports = build_root/'reports/ui'
        reports.mkdir(parents=True, exist_ok=True)
        run = Path(tempfile.mkdtemp(prefix=datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ-'), dir=reports))
        report = {'commit': before['commit'], 'source_sha256': before['source_sha256'], 'synthetic_native_commands': True,
                  'assets': asset_hashes(ROOT/'output/tauri-redunar/ui/dist'), 'suites': {}}
        try:
            for group, names in GROUPS.items():
                for name in names:
                    result = {'group': group, 'status': 'not_run'}
                    report['suites'][name] = result
                    if name not in selected:
                        continue
                    reasons = ([problem] if problem else [])+required[group]
                    if reasons:
                        result.update(status='blocked', reasons=reasons)
                    else:
                        try:
                            result.update(run_suite(name, group, run/name, args.timeout))
                        except KeyboardInterrupt:
                            result.update(status='failed', reason='Suite interrupted')
                            raise
                        except Exception as error:
                            result.update(status='failed', reason=str(error))
                    print(name+': '+result['status'], flush=True)
        finally:
            report['source_unchanged'] = source_snapshot() == before
            report['assets_unchanged'] = asset_hashes(ROOT/'output/tauri-redunar/ui/dist') == report['assets']
            (run/'results.json').write_text(json.dumps(report, indent=2)+'\n')
            print('Report: '+str(run/'results.json'), flush=True)
        statuses = {item['status'] for item in report['suites'].values()}
        if 'failed' in statuses or not report['source_unchanged'] or not report['assets_unchanged']:
            return 1
        return 2 if 'blocked' in statuses else 0


if __name__ == '__main__':
    raise SystemExit(main())
