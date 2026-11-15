#!/usr/bin/env python3
"""Build the frontend and bind its assets to the exact working source."""
import hashlib
import json
import os
from pathlib import Path
import runpy
import subprocess

ROOT = Path(__file__).resolve().parents[1]
NATIVE = ROOT/'output/tauri-redunar'


def asset_hashes(directory):
    return {str(path.relative_to(directory)): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in sorted(directory.rglob('*')) if path.is_file()}


def source_snapshot():
    source_tools = runpy.run_path(str(ROOT/'tools/source-snapshot.py'))
    manifest = os.environ.get('REDUNAR_SOURCE_MANIFEST')
    if manifest:
        return source_tools['verify_snapshot'](ROOT, Path(manifest))
    return source_tools['snapshot'](ROOT)


def main():
    before = source_snapshot()
    subprocess.run([str(NATIVE/'node_modules/.bin/vite'), 'build', '--config', 'vite.config.js', '--outDir', 'dist'],
                   cwd=NATIVE, check=True)
    after = source_snapshot()
    if before != after:
        raise SystemExit('Source changed during the UI build. Rebuild the fixed source tree.')
    report = ROOT/'.redunar-build/ui-build.json'
    report.parent.mkdir(parents=True, exist_ok=True)
    report.write_text(json.dumps({'commit': after['commit'], 'source_sha256': after['source_sha256'],
                                 'assets': asset_hashes(NATIVE/'ui/dist')}, indent=2)+'\n')


if __name__ == '__main__':
    main()
