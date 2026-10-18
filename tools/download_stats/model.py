"""Package filtering and the shared JSON/SVG presentation contract."""
import hashlib
from html import escape
import json

from .history import instant

REPOSITORY = 'shukzi/Redunar'
PACKAGES = frozenset((
    'redunar-app-linux-amd64.deb',
    'redunar-app-linux-opensuse-x86_64.rpm',
    'redunar-app-linux-x86_64.pkg.tar.zst',
    'redunar-app-linux-x86_64.rpm',
    'redunar-app-linux-x86_64.tar.gz',
))


def summarize(releases, checked_at):
    now = instant(checked_at)
    if not isinstance(releases, list):
        raise ValueError('Expected a complete release list.')
    published = [r for r in releases if not r.get('draft')
                 and not r.get('prerelease') and r.get('published_at')]
    if not published:
        raise ValueError('No published stable releases; retain the previous data.')
    selected, seen = [], set()
    for release in published:
        if instant(release['published_at']) > now or not isinstance(release.get('assets'), list):
            raise ValueError('Invalid release timestamp or asset list.')
        for asset in release['assets']:
            if asset.get('name') not in PACKAGES or asset.get('state') != 'uploaded':
                continue
            count, asset_id = asset.get('download_count'), asset.get('id')
            if type(count) is not int or count < 0 or type(asset_id) is not int or asset_id <= 0:
                raise ValueError('Invalid package counter or asset ID.')
            if asset_id in seen or instant(asset['created_at']) > now:
                raise ValueError('Duplicate asset or future creation time.')
            seen.add(asset_id)
            selected.append({'release': release['tag_name'], 'id': asset_id,
                             'name': asset['name'], 'downloads': count,
                             'created_at': asset['created_at']})
    if not selected:
        raise ValueError('No app packages; retain the previous data.')
    return {'repository': REPOSITORY, 'checked_at': checked_at,
            'included_filenames': sorted(PACKAGES),
            'first_release_at': min((r['published_at'] for r in published), key=instant),
            'total': sum(a['downloads'] for a in selected), 'assets': selected}


def fingerprint(document):
    # Both consumers can identify exactly the same immutable presentation.
    body = {k: v for k, v in document.items() if k != 'snapshot_id'}
    return hashlib.sha256(json.dumps(body, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def weekly_document(history, snapshot):
    # Keep the existing Python entry point and data URLs compatible. The
    # presentation now reports total observed downloads, including the baseline.
    total = sum(asset['downloads'] for asset in history['assets'].values())
    display = {'kind': 'lifetime', 'value': total, 'number': f'{total:,}',
               'badge_number': f'{total:,}', 'label': 'Total downloads',
               'badge_label': 'Total downloads',
               'title': 'Total observed app-package downloads across published stable releases. Includes updates and repeat downloads, not unique people or confirmed installs.'}
    result = {'schema_version': 1, 'repository': REPOSITORY,
              'checked_at': snapshot['checked_at'], 'tracking_started_at': history['started_at'],
              'included_filenames': sorted(PACKAGES),
              'recorded_downloads': history['observations'][-1]['recorded_downloads'],
              'lifetime_downloads': total, 'display': display,
              'policy': 'Published stable app packages only; includes updates and repeat downloads, not unique users or confirmed installs.'}
    result['snapshot_id'] = fingerprint(result)
    return result


def badge(document):
    display = document['display']
    value = display['badge_number']
    left = max(116, len(display['badge_label']) * 7 + 16)
    right = max(34, len(value) * 8 + 16)
    width = left + right
    title = escape(display['title'], quote=True)
    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="22" viewBox="0 0 {width} 22" role="img" aria-label="{title}">
<title>{title}</title><metadata>sha256:{document['snapshot_id']}</metadata>
<defs><clipPath id="badge"><rect width="{width}" height="22" rx="5"/></clipPath></defs>
<g clip-path="url(#badge)"><path fill="#171a1e" d="M0 0h{left}v22H0z"/><path fill="#672536" d="M{left} 0h{right}v22H{left}z"/></g>
<rect x=".5" y=".5" width="{width-1}" height="21" rx="4.5" fill="none" stroke="#5b606944"/>
<g font-family="DejaVu Sans,Arial,sans-serif" font-size="11" text-anchor="middle"><text x="{left/2}" y="15" fill="#bdc3cc">{escape(display['badge_label'])}</text><text x="{left+right/2}" y="15" fill="#fff0f2">{escape(value)}</text></g></svg>
'''


def bundle(history, snapshot):
    document = weekly_document(history, snapshot)
    return {'history.json': json.dumps(history, indent=2) + '\n',
            'weekly.json': json.dumps(document, indent=2) + '\n',
            'weekly.svg': badge(document)}
