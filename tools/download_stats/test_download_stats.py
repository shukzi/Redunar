"""Offline regressions for counters, continuity and publication boundaries."""
from copy import deepcopy
from contextlib import redirect_stderr, redirect_stdout
from datetime import datetime, timezone
import base64
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from urllib.error import HTTPError

from tools.download_stats import github, history, model, update


def sample(day=18, count=98, assets=None):
    assets = assets if assets is not None else [
        {'id': 1, 'name': sorted(model.PACKAGES)[0], 'downloads': count,
         'created_at': '2026-09-20T10:00:00Z', 'release': 'v1'}]
    return {'repository': model.REPOSITORY, 'checked_at': f'2026-10-{day:02}T10:00:00Z',
            'first_release_at': '2026-09-20T10:00:00Z',
            'included_filenames': sorted(model.PACKAGES),
            'assets': assets, 'total': sum(a['downloads'] for a in assets)}


class CounterTests(unittest.TestCase):
    def test_seed_is_preserved_and_not_new_downloads(self):
        seed = update.validate_history(update.seed())
        self.assertEqual(seed['started_at'], '2026-10-18T10:44:18Z')
        self.assertEqual(len(seed['assets']), 115)
        self.assertEqual(seed['observations'][0]['github_lifetime_total'], 98)
        self.assertEqual(seed['observations'][-1]['recorded_downloads'], 0)
        with tempfile.TemporaryDirectory() as output:
            update.main(['--baseline-only', '--output', output])
            generated = json.loads((Path(output) / 'history.json').read_text())
            self.assertEqual(generated, seed)

    def test_bad_seed_prefix_and_regressed_registry_are_rejected(self):
        for mutate in ('prefix', 'registry', 'time'):
            seed = deepcopy(update.seed())
            if mutate == 'prefix':
                seed['observations'][0]['recorded_downloads'] = 98
            elif mutate == 'registry':
                seed['assets'][next(iter(seed['assets']))]['downloads'] = -1
            else:
                seed['observations'].append(deepcopy(seed['observations'][-1]))
            with self.subTest(mutate=mutate), self.assertRaises(ValueError):
                update.validate_history(seed)

    def test_package_allowlist_all_stable_releases_and_no_metadata(self):
        assets = [{'id': i + 1, 'name': name, 'state': 'uploaded', 'download_count': 2,
                   'created_at': '2026-10-17T00:00:00Z'} for i, name in enumerate(sorted(model.PACKAGES))]
        extra = [{**assets[0], 'id': 30, 'name': 'SHA256SUMS', 'download_count': 100},
                 {**assets[0], 'id': 31, 'name': assets[0]['name'] + '.sha256'},
                 {**assets[0], 'id': 32, 'name': 'source.tar.gz'},
                 {**assets[0], 'id': 33, 'state': 'new'}]
        release = {'tag_name': 'v1', 'published_at': '2026-10-17T00:00:00Z', 'assets': assets + extra}
        older = {**release, 'tag_name': 'v0', 'assets': [{**assets[0], 'id': 40, 'download_count': 3}]}
        rows = [release, older, {**release, 'draft': True}, {**release, 'prerelease': True}]
        self.assertEqual(model.summarize(rows, sample()['checked_at'])['total'], 13)
        for bad in (-1, True, 1.5, None):
            with self.subTest(count=bad), self.assertRaises(ValueError):
                model.summarize([{**release, 'assets': [{**assets[0], 'download_count': bad}]}], sample()['checked_at'])
        with self.assertRaises(ValueError):
            model.summarize([release, release], sample()['checked_at'])

    def test_new_deleted_and_rediscovered_assets_do_not_reset_deltas(self):
        recorded = history.record(sample())
        recorded = history.record(sample(19, 101), recorded)
        new = {'id': 2, 'name': sorted(model.PACKAGES)[1], 'downloads': 4,
               'created_at': '2026-10-19T12:00:00Z', 'release': 'v2'}
        recorded = history.record(sample(20, assets=[new]), recorded)
        self.assertEqual(recorded['observations'][-1]['recorded_downloads'], 7)
        self.assertEqual(model.weekly_document(recorded, sample(20, assets=[new]))['lifetime_downloads'], 105)
        old = {**new, 'id': 3, 'downloads': 500, 'created_at': '2026-09-01T00:00:00Z'}
        recorded = history.record(sample(21, assets=[new, old]), recorded)
        recorded = history.record(sample(22, 103), recorded)
        self.assertEqual(recorded['observations'][-1]['recorded_downloads'], 9)
        self.assertIn('2', recorded['assets'])
        self.assertEqual(history.record(sample(22, 103), recorded), recorded)
        for bad in (sample(21, 104), sample(23, 97), sample(23, assets=[])):
            with self.assertRaises(ValueError):
                history.record(bad, recorded)

    def test_total_includes_baseline_without_estimates_or_weekly_coverage(self):
        recorded = history.record(sample())
        display = model.weekly_document(recorded, sample())['display']
        self.assertEqual((display['number'], display['label']), ('98', 'Total downloads'))
        for day in range(19, 27):
            snapshot = sample(day, 98 + (day - 18) * 3)
            recorded = history.record(snapshot, recorded)
        display = model.weekly_document(recorded, snapshot)['display']
        self.assertEqual((display['kind'], display['value']), ('lifetime', 122))
        gap = history.record(sample(26, 110), history.record(sample()))
        self.assertEqual(model.weekly_document(gap, sample(26, 110))['display']['value'], 110)

    def test_svg_and_json_share_the_snapshot_and_value(self):
        files = model.bundle(history.record(sample()), sample())
        doc = json.loads(files['weekly.json'])
        self.assertEqual(doc['snapshot_id'], model.fingerprint(doc))
        self.assertIn('sha256:' + doc['snapshot_id'], files['weekly.svg'])
        self.assertIn('>98</text>', files['weekly.svg'])
        self.assertIn('>Total downloads</text>', files['weekly.svg'])
        self.assertEqual(doc['lifetime_downloads'], doc['display']['value'])


class ApiTests(unittest.TestCase):
    def test_pagination_and_embedded_asset_pagination(self):
        client = github.Client()
        with patch.object(client, 'request', side_effect=[[{}] * 100, [{'id': 101}]]) as fetch:
            self.assertEqual(len(client.pages('/releases')), 101)
            self.assertIn('page=2', fetch.call_args.args[0])
        with patch.object(client, 'pages', side_effect=[[{'id': 1, 'assets': [{}] * 100}], [{'id': 2}]]) as pages:
            self.assertEqual(client.releases()[0]['assets'], [{'id': 2}])
            self.assertEqual(pages.call_args.args[0], '/releases/1/assets')

    def test_api_failure_is_not_zero_or_bootstrap(self):
        client = github.Client()
        error = HTTPError('https://api.github.com/', 403, 'forbidden', {}, None)
        self.addCleanup(error.close)
        with patch.object(client, 'request', side_effect=error):
            with self.assertRaises(HTTPError):
                github.load_remote(client)
        with patch.object(client, 'request', side_effect=[{'message': 'unavailable'}]):
            with self.assertRaises(ValueError):
                client.pages('/releases')

    def test_only_missing_ref_is_unseeded_not_missing_history(self):
        client = github.Client()
        error = HTTPError('https://api.github.com/', 404, 'missing', {}, None)
        self.addCleanup(error.close)
        with patch.object(client, 'request', side_effect=error):
            self.assertEqual(github.load_remote(client), (None, None))
        with patch.object(client, 'request', side_effect=[{'object': {'sha': 'head'}}, error]):
            with self.assertRaises(HTTPError):
                github.load_remote(client)

    def test_remote_data_branch_loads_only_data_not_code(self):
        client = github.Client()
        blob = base64.b64encode(json.dumps(update.seed()).encode()).decode()
        tree = [{'path': name, 'sha': name, 'type': 'blob', 'mode': '100644'} for name in github.FILES]
        replies = [{'object': {'sha': 'head'}}, {'tree': {'sha': 'tree'}}, {'tree': tree},
                   {'encoding': 'base64', 'content': blob}]
        with patch.object(client, 'request', side_effect=replies):
            self.assertEqual(github.load_remote(client), ('head', update.seed()))
        replies[2] = {'tree': tree + [{'path': 'unexpected.py', 'type': 'blob', 'mode': '100644'}]}
        with patch.object(client, 'request', side_effect=replies), self.assertRaises(ValueError):
            github.load_remote(client)


class PublicationTests(unittest.TestCase):
    def test_one_commit_and_nonforced_ref_advance(self):
        client = github.Client()
        files = {name: 'fixture' for name in github.FILES}
        for head in (None, 'old'):
            with self.subTest(head=head), patch.object(client, 'request', side_effect=[{'sha': 'tree'}, {'sha': 'new'}, {}]) as api:
                github.publish(client, files, head)
                commit = api.call_args_list[1].args[2]
                self.assertEqual(commit['parents'], [head] if head else [])
                self.assertEqual(commit['message'], github.UPDATE_MESSAGE if head else github.INITIAL_MESSAGE)
                ref = api.call_args_list[-1].args
                self.assertEqual(ref[0], '/git/refs/heads/download-stats' if head else '/git/refs')
                if head:
                    self.assertIs(ref[2]['force'], False)
                paths = {row['path'] for row in api.call_args_list[0].args[2]['tree']}
                self.assertEqual(paths, github.FILES)

    def test_rejected_ref_never_retries_with_force(self):
        client = github.Client()
        error = HTTPError('https://api.github.com/', 422, 'not fast forward', {}, None)
        self.addCleanup(error.close)
        with patch.object(client, 'request', side_effect=[{'sha': 'tree'}, {'sha': 'new'}, error]) as api:
            with self.assertRaises(HTTPError):
                github.publish(client, {name: 'fixture' for name in github.FILES}, 'old')
            self.assertEqual(api.call_count, 3)

    def test_explicit_activation_and_no_fixture_publication(self):
        with patch.dict('os.environ', {}, clear=True), patch.object(github.Client, 'request') as api, redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit):
                update.main(['--publish'])
            api.assert_not_called()
        with redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            update.main(['--publish', '--baseline-only'])

    def test_bootstrap_is_explicit_and_scheduled_runs_do_not_reseed(self):
        enabled = {'GITHUB_ACTIONS': 'true', 'GITHUB_REPOSITORY': model.REPOSITORY,
                   'GITHUB_REF': 'refs/heads/main', 'REDUNAR_DOWNLOAD_STATS_ENABLED': 'true',
                   'GH_TOKEN': 'fixture-token-not-a-real-secret'}
        with patch.dict('os.environ', enabled, clear=True), patch.object(update, 'publish') as publish:
            with patch.object(update, 'load_remote', return_value=(None, None)):
                with self.assertRaisesRegex(ValueError, 'manual bootstrap'):
                    update.main(['--publish'])
            with patch.object(update, 'load_remote', return_value=('head', update.seed())):
                with self.assertRaisesRegex(ValueError, 'exists'):
                    update.main(['--publish', '--bootstrap'])
            publish.assert_not_called()

    def test_api_failure_never_writes_remote_data(self):
        enabled = {'GITHUB_ACTIONS': 'true', 'GITHUB_REPOSITORY': model.REPOSITORY,
                   'GITHUB_REF': 'refs/heads/main', 'REDUNAR_DOWNLOAD_STATS_ENABLED': 'true',
                   'GH_TOKEN': 'fixture-token-not-a-real-secret'}
        with patch.dict('os.environ', enabled, clear=True), patch.object(update, 'publish') as publish:
            with patch.object(update, 'load_remote', return_value=('head', update.seed())), patch.object(github.Client, 'releases', side_effect=OSError('offline')):
                with self.assertRaises(OSError):
                    update.main(['--publish'])
            publish.assert_not_called()

    def test_bootstrap_to_continuation_keeps_the_same_baseline(self):
        enabled = {'GITHUB_ACTIONS': 'true', 'GITHUB_REPOSITORY': model.REPOSITORY,
                   'GITHUB_REF': 'refs/heads/main', 'REDUNAR_DOWNLOAD_STATS_ENABLED': 'true',
                   'GH_TOKEN': 'fixture-token-not-a-real-secret'}
        assets = [{**row, 'state': 'uploaded', 'download_count': row['downloads']}
                  for row in update.seed()['assets'].values()]
        assets[0]['download_count'] += 3
        releases = [{'tag_name': 'fixture', 'published_at': '2026-09-20T11:55:48Z', 'assets': assets}]
        with tempfile.TemporaryDirectory() as output, patch.dict('os.environ', enabled, clear=True), patch.object(github.Client, 'releases', return_value=releases), patch.object(update, 'publish') as publish, redirect_stdout(io.StringIO()):
            with patch.object(update, 'load_remote', return_value=(None, None)), patch.object(update, 'datetime') as clock:
                clock.now.return_value = datetime(2026, 10, 19, 10, 47, tzinfo=timezone.utc)
                update.main(['--publish', '--bootstrap', '--output', output])
            files, head = publish.call_args.args[1:]
            self.assertIsNone(head)
            recorded = json.loads(files['history.json'])
            self.assertEqual(recorded['observations'][-1]['recorded_downloads'], 3)
            self.assertEqual(recorded['observations'][:2], update.seed()['observations'])
            with patch.object(update, 'load_remote', return_value=('existing', recorded)), patch.object(update, 'datetime') as clock:
                clock.now.return_value = datetime(2026, 10, 20, 10, 47, tzinfo=timezone.utc)
                update.main(['--publish', '--output', output])
            files, head = publish.call_args.args[1:]
            self.assertEqual(head, 'existing')
            final = json.loads(files['history.json'])
            self.assertEqual(final['observations'][-1]['recorded_downloads'], 3)
            self.assertEqual(final['started_at'], update.seed()['started_at'])


if __name__ == '__main__':
    unittest.main()
