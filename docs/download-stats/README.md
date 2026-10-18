# Weekly package download statistics

This recorder produces one presentation snapshot for the website and README
badge. It counts only uploaded app packages from all published stable Redunar
releases. The five exact filenames are defined in
[`model.py`](../../tools/download_stats/model.py). Checksums, signatures, source
archives, metadata files, drafts and prereleases do not count. Updates and repeat
downloads do count; the metric cannot identify installations or unique people.

## Data and baseline

The original recording began October 18, 2026 at 10:44:18 UTC. The 98 existing
lifetime downloads are a baseline, not new downloads. The checked-in seed files
preserve the original asset IDs and two observations. They contain only public
package metadata and aggregate counters, with no visitor or owner diagnostics.

During the first week, `Weekly downloads` shows the historical average with an
approximation symbol and an explanatory tooltip. After a full recorded week,
daily observations provide a rolling seven-day count. Up to one daily interval
at the window boundary is unresolved. A coverage gap longer than 36 hours falls
back to observed downloads since recording began rather than inventing a week.

Newly uploaded packages count from their first observation; newly discovered
packages that predate the baseline are baselined conservatively. Deleted assets
remain in the history, although downloads between their last observation and
deletion cannot be recovered. Counter regressions require investigation rather
than silently subtracting or resetting downloads.

The `download-stats` branch contains exactly three files in one Git commit:

- `history.json`: durable per-asset counters and observations.
- `weekly.json`: the shared public count, labels, timestamp, policy and snapshot ID.
- `weekly.svg`: the README badge generated from that same JSON presentation.

The JSON snapshot ID also appears in the SVG metadata. Website builds import the
JSON value and labels directly; they must not calculate a separate weekly count.
The website remains a static snapshot until rebuilt. This workflow does not
deploy the website or add browser-side API requests.

## Local checks and preview

Python 3.10 or newer and its standard library are sufficient. From the repository
root:

```sh
python3 -m unittest discover -s tools/download_stats -p 'test_*.py'
python3 -m tools.download_stats.update --baseline-only --output .redunar-build/download-stats
# Optional read-only refresh using GitHub's public API:
python3 -m tools.download_stats.update --output .redunar-build/download-stats
```

The default is local generation; it never writes to GitHub. To replay a complete
previously fetched release list, use `--from-json FILE --checked-at TIMESTAMP`.
An optional `--history FILE` continues a local recording. Keep its resulting
history for subsequent runs; starting repeatedly from the seed does not create
a continuing daily series.

## Workflow operation

The workflow is inactive unless the repository variable
`REDUNAR_DOWNLOAD_STATS_ENABLED` equals `true`. Once deliberately enabled, it
runs daily at 10:47 UTC and can be run manually from `main`. Its first run needs
the explicit manual `bootstrap` input to create the data-only branch from the
reviewed seed. A scheduled run never recreates missing history. Existing data
cannot be overwritten with bootstrap.

Only the named upstream repository and `main` may publish. The job uses the
short-lived `GITHUB_TOKEN`, with `contents: write` for the recording job and no
personal token. That permission applies to repository contents, not just one
branch; the publisher limits its writes to `download-stats` and the three files.
Checkout is pinned to a full commit and does not retain Git credentials. No
release, app package, source branch, comment or Cloudflare deployment is edited.
Concurrent writers fail without force-pushing. All new data becomes visible
together when the branch reference advances; errors preserve the last published
snapshot. A failed ref update can leave an unreachable Git object, not a partial
published snapshot.

GitHub can delay or drop scheduled runs; this is daily observation rather than
an exact-time service. Scheduled workflows in an inactive public repository can
also be disabled by GitHub. Review workflow failures and coverage gaps. Keep
history in Git rather than relying on expiring workflow artifacts or caches.
Disable the workflow to stop recording; retain its branch and seed to preserve
continuity. Nothing is installed into the app or used to track visitors.

## README badge

Once the data branch exists, the proposed README image is:

```markdown
[![Weekly downloads](https://raw.githubusercontent.com/shukzi/Redunar/download-stats/weekly.svg)](https://github.com/shukzi/Redunar/releases)
```

GitHub's image proxy can cache a README badge, so its displayed number may lag
the JSON briefly. The timestamp and snapshot ID identify the underlying data.
There is no independent badge counter.

## References

- [GitHub release-asset counters](https://docs.github.com/en/rest/releases/assets)
- [Scheduled workflow behavior](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#schedule)
- [Workflow token permissions](https://docs.github.com/en/actions/tutorials/authenticate-with-github_token)
