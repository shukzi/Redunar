"""Public asset-counter observations; no visitor data or network operations."""
from copy import deepcopy
from datetime import datetime, timedelta, timezone
from zoneinfo import ZoneInfo

WINDOW = timedelta(days=7)
MAX_SAMPLE_GAP = timedelta(hours=36)


def instant(value):
    result = datetime.fromisoformat(value.replace('Z', '+00:00'))
    if result.tzinfo is None:
        raise ValueError('Counter observations require an explicit timezone.')
    return result.astimezone(timezone.utc)


def record(snapshot, history=None):
    """Count positive changes by asset ID, without resetting deleted assets."""
    now = instant(snapshot['checked_at'])
    if not snapshot['assets']:
        raise ValueError('No app assets in response; keep the previous history.')
    if history is None:
        history = {'schema_version': 1, 'repository': snapshot['repository'],
                   'started_at': snapshot['checked_at'],
                   'included_filenames': snapshot['included_filenames'],
                   'assets': {}, 'observations': []}
    else:
        if (history.get('schema_version') != 1 or history.get('repository') != snapshot['repository']
                or history.get('included_filenames') != snapshot['included_filenames']):
            raise ValueError('Tracking policy changed; do not silently reset the baseline.')
        history = deepcopy(history)
    observations = history['observations']
    if observations and now <= instant(observations[-1]['checked_at']):
        # Retries must neither duplicate downloads nor overwrite a newer sample.
        if now == instant(observations[-1]['checked_at']) and all(
                str(a['id']) in history['assets'] and a['downloads'] == history['assets'][str(a['id'])]['downloads']
                for a in snapshot['assets']):
            return history
        raise ValueError('Observation is older than the saved history.')
    baseline = not observations
    cumulative = observations[-1]['recorded_downloads'] if observations else 0
    changes = []
    for asset in snapshot['assets']:
        key = str(asset['id'])
        previous = history['assets'].get(key)
        if previous and (previous['name'] != asset['name'] or asset['downloads'] < previous['downloads']):
            raise ValueError('Asset identity or counter regressed; retain the previous sample.')
        if previous:
            increment = asset['downloads'] - previous['downloads']
        else:
            # Newly uploaded packages can start above zero before our next run.
            # Newly discovered OLD packages are baselined, not backdated.
            created = instant(asset['created_at'])
            increment = asset['downloads'] if not baseline and created >= instant(history['started_at']) else 0
        if increment:
            changes.append({'asset_id': asset['id'], 'downloads': increment})
            cumulative += increment
        history['assets'][key] = {**asset, 'first_seen': previous['first_seen'] if previous else snapshot['checked_at']}
    observations.append({'checked_at': snapshot['checked_at'],
                         'recorded_downloads': cumulative,
                         'github_lifetime_total': snapshot['total'], 'changes': changes})
    return history


def period(history, snapshot):
    observations = history['observations']
    first, last = observations[0], observations[-1]
    now, started = instant(last['checked_at']), instant(first['checked_at'])
    cutoff = now - WINDOW
    anchor = next((i for i, row in enumerate(observations) if instant(row['checked_at']) >= cutoff), None)
    covered = observations[anchor:] if anchor is not None else []
    # Daily observations cannot resolve downloads inside the boundary interval.
    # Use only intervals wholly inside the seven-day window, never invent an
    # interpolation. The tooltip exposes that daily-observation boundary.
    weekly = (now - started >= WINDOW and covered
              and instant(covered[0]['checked_at']) <= cutoff + timedelta(days=1)
              and all(instant(b['checked_at']) - instant(a['checked_at']) <= MAX_SAMPLE_GAP
                      for a, b in zip(covered, covered[1:])))
    if weekly:
        value = last['recorded_downloads'] - covered[0]['recorded_downloads']
        return {'kind': 'observed-week', 'value': value,
                'label': 'Weekly downloads', 'badge_label': 'Weekly downloads',
                'window_started_at': covered[0]['checked_at'],
                'title': 'App-package downloads observed in the past 7 days, based on daily snapshots; includes updates and repeat downloads. The boundary can differ by up to one daily sample.'}
    # Until a full week's observations exist, a historical average is clearly
    # labelled as an estimate. It is not a made-up last-week count or user count.
    age = (now - instant(snapshot['first_release_at'])).total_seconds() / 86400
    if age > 0 and now - started < WINDOW:
        value = int(snapshot['total'] * 7 / age + .5)
        since = started.astimezone(ZoneInfo('Europe/Amsterdam')).strftime('%d %b').lstrip('0')
        return {'kind': 'historical-weekly-average', 'value': value,
                'label': 'Weekly downloads', 'badge_label': 'Weekly downloads',
                'window_started_at': snapshot['first_release_at'],
                'title': f"Estimated historical average: {snapshot['total']} app-package downloads since the first published release, divided by elapsed weeks. Includes updates and repeats. Weekly recording started {since}; not a measured last-week or unique-user count."}
    since = started.astimezone(ZoneInfo('Europe/Amsterdam')).strftime('%d %b').lstrip('0')
    return {'kind': 'observed-since-start', 'value': last['recorded_downloads'],
            'label': 'downloads since ' + since, 'badge_label': 'since ' + since,
            'window_started_at': first['checked_at'],
            'title': 'Observed app-package downloads since recording began; a complete recent week is not available. Includes updates and repeat downloads.'}
