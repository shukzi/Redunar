"""Generate locally by default; publication requires explicit workflow activation."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path

from .github import Client, load_remote, publish
from .history import instant, record
from .model import REPOSITORY, bundle, summarize

ROOT = Path(__file__).resolve().parents[2]
SEEDS = ROOT / 'docs/download-stats'


def seed():
    return json.loads((SEEDS / 'baseline-history.json').read_text())


def validate_history(history):
    # A remote history may not silently replace our original starting point.
    baseline = seed()
    prefix = baseline['observations']
    if (history.get('schema_version') != 1 or history.get('repository') != REPOSITORY
            or history.get('started_at') != baseline['started_at']
            or history.get('included_filenames') != baseline['included_filenames']
            or history.get('observations', [])[:len(prefix)] != prefix
            or not set(baseline['assets']).issubset(history.get('assets', {}))):
        raise ValueError('History does not preserve the approved original baseline.')
    previous_time, previous_count = None, 0
    for observation in history['observations']:
        timestamp = instant(observation['checked_at'])
        count = observation['recorded_downloads']
        if (type(count) is not int or count < previous_count
                or previous_time is not None and timestamp <= previous_time
                or sum(change['downloads'] for change in observation['changes']) != count - previous_count):
            raise ValueError('Invalid cumulative observation sequence.')
        previous_time, previous_count = timestamp, count
    for key, original in baseline['assets'].items():
        current = history['assets'][key]
        if (current['id'] != original['id'] or current['name'] != original['name']
                or current['created_at'] != original['created_at']
                or type(current['downloads']) is not int or current['downloads'] < original['downloads']):
            raise ValueError('A baseline asset was replaced or its counter regressed.')
    return history


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--from-json', type=Path, help='Complete release list for an offline dry run')
    parser.add_argument('--checked-at', help='Original fetch time; required with --from-json')
    parser.add_argument('--history', type=Path, help='Local history; defaults to the original baseline')
    parser.add_argument('--output', type=Path, default=Path('.redunar-build/download-stats'))
    parser.add_argument('--baseline-only', action='store_true', help='Render original baseline without any API access')
    parser.add_argument('--publish', action='store_true', help='Write the enabled GitHub Actions data branch')
    parser.add_argument('--bootstrap', action='store_true', help='Explicitly initialize a missing data branch once')
    args = parser.parse_args(argv)
    if args.from_json and not args.checked_at:
        parser.error('--from-json requires its actual --checked-at time')
    if args.bootstrap and not args.publish:
        parser.error('--bootstrap requires --publish')
    if args.publish and (args.from_json or args.history or args.baseline_only or args.checked_at):
        parser.error('Publication must use live counters and the remote history.')
    if args.publish and not (
            os.environ.get('GITHUB_ACTIONS') == 'true'
            and os.environ.get('GITHUB_REPOSITORY') == REPOSITORY
            and os.environ.get('GITHUB_REF') == 'refs/heads/main'
            and os.environ.get('REDUNAR_DOWNLOAD_STATS_ENABLED') == 'true'
            and os.environ.get('GH_TOKEN')):
        parser.error('Publication is restricted to the explicitly enabled main-branch workflow.')
    client, head = Client(os.environ.get('GH_TOKEN') if args.publish else None), None
    if args.publish:
        head, history = load_remote(client)
        if history is None:
            if not args.bootstrap:
                raise ValueError('Missing data branch: manual bootstrap is required; never reset history.')
            history = seed()
        elif args.bootstrap:
            raise ValueError('The data branch exists; bootstrap cannot overwrite it.')
    else:
        history = json.loads(args.history.read_text()) if args.history else seed()
    validate_history(history)
    if args.baseline_only:
        snapshot = json.loads((SEEDS / 'baseline-snapshot.json').read_text())
    else:
        releases = json.loads(args.from_json.read_text()) if args.from_json else client.releases()
        checked = args.checked_at or datetime.now(timezone.utc).isoformat(timespec='seconds')
        snapshot = summarize(releases, checked)
        history = record(snapshot, history)
    files = bundle(history, snapshot)
    args.output.mkdir(parents=True, exist_ok=True)
    for name, content in files.items():
        (args.output / name).write_text(content)
    if args.publish:
        publish(client, files, head)
        print('Published the shared download snapshot.')
    else:
        print('Local snapshot generated; nothing published.')


if __name__ == '__main__':
    main()
