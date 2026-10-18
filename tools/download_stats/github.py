"""Bounded GitHub API access and atomic publication on a data-only branch."""
import base64
import json
from urllib.error import HTTPError
from urllib.request import HTTPRedirectHandler, Request, build_opener

from .model import REPOSITORY

API = f'https://api.github.com/repos/{REPOSITORY}'
BRANCH = 'download-stats'
FILES = frozenset(('history.json', 'weekly.json', 'weekly.svg'))
INITIAL_MESSAGE = 'chore: initialize download statistics'
UPDATE_MESSAGE = 'chore: refresh download statistics'
BOT = {'name': 'github-actions[bot]',
       'email': '41898282+github-actions[bot]@users.noreply.github.com'}


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, request, *args, **kwargs):
        # Never forward a workflow token to a redirect target.
        return None


class Client:
    def __init__(self, token=None):
        self.token = token
        self.opener = build_opener(NoRedirect())

    def request(self, path, method='GET', body=None):
        if not path.startswith('/') or path.startswith('//'):
            raise ValueError('Expected a repository-relative API path.')
        headers = {'Accept': 'application/vnd.github+json', 'User-Agent': 'Redunar-download-stats'}
        if self.token:
            headers['Authorization'] = 'Bearer ' + self.token
        data = None
        if body is not None:
            data = json.dumps(body).encode()
            headers['Content-Type'] = 'application/json'
        request = Request(API + path, method=method, data=data, headers=headers)
        with self.opener.open(request, timeout=30) as response:
            raw = response.read(16 * 1024 * 1024 + 1)
        if len(raw) > 16 * 1024 * 1024:
            raise ValueError('API response exceeds the recording limit.')
        return json.loads(raw)

    def pages(self, path):
        result = []
        for page in range(1, 1001):
            rows = self.request(f'{path}?per_page=100&page={page}')
            if not isinstance(rows, list):
                raise ValueError('Incomplete API response; retain previous data.')
            result.extend(rows)
            if len(rows) < 100:
                return result
        raise ValueError('Pagination limit reached; retain previous data.')

    def releases(self):
        releases = self.pages('/releases')
        for release in releases:
            if not release.get('draft') and not release.get('prerelease') and len(release.get('assets', [])) >= 100:
                release['assets'] = self.pages(f"/releases/{int(release['id'])}/assets")
        return releases


def load_remote(client):
    """Return the expected ref and history; only a missing ref means unseeded."""
    try:
        ref = client.request('/git/ref/heads/' + BRANCH)
    except HTTPError as error:
        if error.code == 404:
            return None, None
        raise
    head = ref['object']['sha']
    commit = client.request('/git/commits/' + head)
    tree = client.request('/git/trees/' + commit['tree']['sha'])
    entries = {row['path']: row for row in tree['tree']}
    if tree.get('truncated') or set(entries) != FILES or any(
            row['type'] != 'blob' or row['mode'] != '100644' for row in entries.values()):
        raise ValueError('Unexpected data-branch contents; no automatic overwrite.')
    blob = client.request('/git/blobs/' + entries['history.json']['sha'])
    if blob.get('encoding') != 'base64':
        raise ValueError('Unsupported history encoding.')
    history = json.loads(base64.b64decode(blob['content']).decode())
    return head, history


def publish(client, files, expected_head):
    if set(files) != FILES:
        raise ValueError('Publication may contain only the three data files.')
    tree = client.request('/git/trees', 'POST', {'tree': [
        {'path': name, 'mode': '100644', 'type': 'blob', 'content': files[name]}
        for name in sorted(FILES)]})
    commit = client.request('/git/commits', 'POST', {
        'message': UPDATE_MESSAGE if expected_head else INITIAL_MESSAGE,
        'tree': tree['sha'], 'parents': [expected_head] if expected_head else [],
        'author': BOT, 'committer': BOT})
    # A normal fast-forward refuses a concurrent writer; never force-push.
    if expected_head:
        client.request('/git/refs/heads/' + BRANCH, 'PATCH', {'sha': commit['sha'], 'force': False})
    else:
        client.request('/git/refs', 'POST', {'ref': 'refs/heads/' + BRANCH, 'sha': commit['sha']})
    return commit['sha']
