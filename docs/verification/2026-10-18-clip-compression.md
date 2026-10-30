# Clip compression and selection exports — October 18, 2026

Local implementation based on `825012d`, on `feat/clip-compression`.
The SHA-256 of the sorted runtime-file hash manifest in the private acceptance
report is `b475fd09d4035c08cf01857ceccacaa0fd3bb213eba88d9916ad9d64d1aa6368`.
This identifies the uncommitted implementation, rather than claiming it is part
of the base commit.

## Behavior

Clip actions offers Compress clip with the approved Size first dialog. Export
selection keeps its trimmed range, with compression inside a collapsed Compress
foldout. Both support MP4/MKV, source-container defaults, editable filename stems,
shared Precision controls and automatic source resolution/video cadence.

Native export owns typed options, decimal size validation, whole-clip duration
inspection, bounded software encoding, final size/duration/audio checks and
exclusive commit. Two-pass encoding budgets all audio streams and mux overhead;
size overshoots retry within the existing deadline. A range already below its
ceiling tries the normal quality encoder first. Outputs are complete media,
never truncated to satisfy a cap. Existing recordings and quota-protected clips
are retained. Cancellation, stale progress, Retry, shutdown and restart admission
keep their explicit ownership boundaries.

## Initial build checks

- `tools/validate.sh full` passed on the final runtime code: syntax/format,
  root and separate Tauri checks/tests, production UI build, frontend tests and
  strict Clippy. Tauri reported 126 passing tests and two ignored fixtures.
- Ten focused native export tests passed after expanding generated-media coverage
  for the normal-quality and two-pass limited paths. Fixtures exercised MP4/MKV
  inputs and outputs, multiple audio tracks, complete decode, trimmed duration,
  source retention, cancellation/child reaping and temporary cleanup.
- Store fixtures passed named/generated MKV identity, reopening, bounded commit,
  container mismatch, existing-name rejection and trash/restore checks. Existing
  MP4 and quota preservation tests remain in place.
- The production WebKit Replay fixture passed, including the Compress foldout,
  retained size/format/range on Retry, whole-clip boundaries, narrow layout,
  early cancellation and rejection of stale status responses. Desktop, narrow
  and folded export screenshots were inspected.
- `python3 output/tauri-redunar/build-native.py` built the optimized x86_64 Linux
  app, both capture libraries, Steam wrapper and shortcut/update helpers locally.
  Adjacent capture components match their generated digest manifest. The app
  executable SHA-256 is
  `ea66daa6e0851518c369cbcff9224b1e320488134b75629ef0ecd826292fa5a8`.
  ELF/dependency inspection found no missing local dynamic libraries.

Logs, source/build hashes and the local launch script are retained under the
checkout-owned `.redunar-build/` directory. The acceptance report is
`.redunar-build/reports/clip-compression/acceptance.json`.

## Review and limitations

A separate local review pass traced UI submission through native admission,
encoding and the store commit, including errors, cancellation, shutdown,
restart, format identity and persisted readers. Strict lint findings in the new
store helper were corrected; delayed UI status responses were guarded by an
export generation before the successful final checks.

Affected docs and references were reconciled. No dependencies, third-party
assets or version metadata changed. Existing MP4 APIs, compatibility readers,
unrelated styles and legal notices were retained; no files were deleted.

No installation, real recording/game test or publication
was performed. The initial local build was subsequently opened at the owner's
request, with its fresh process executable verified against the build digest.
This is a local test build, not an installed-runtime or release
qualification claim. Low upload limits can reduce quality or be impossible for
a duration/audio combination; these cases fail without publishing partial media.

## Size, sidebar and approved motion follow-up

The same task branch adds measured original size in the form and progress view,
and actual output bytes during encoding. Analysis hides any preceding attempt's
bytes; completion uses the final committed size. Retry retains source details.
The draft bar follows the sidebar width with shared 220 ms timing. The approved
Soft fade uses 180 ms arrival and 160 ms decoded-thumbnail fades, with 220 ms
layout settling, retained playback/selection/focus/scroll, stable error
placeholders and reduced-motion handling. Initial lists and filter redraws do
not replay arrival effects.

The production Replay fixture passed original/live size, analysis/reset,
stale progress and cancellation checks. Workspace passed 152 checks including
both sidebar widths with a retained draft. Inventory passed 13 checks including
arrival/thumbnail fades, failed decode and reduced motion. A reduced-motion
fixture exposed a stale retained media-query result; fresh preference reads
resolved it before the successful rerun. All fixtures use private sample state.

Fresh validation and native build evidence for this follow-up is recorded
separately in `.redunar-build/reports/clip-compression/acceptance-size-motion.json`.
The initially opened app process does not acquire subsequent build changes.

The follow-up runtime source manifest SHA-256 is
`cd98c9ebd341563f8e8de0b60ad4440de34c2830b4d9c1112c4ff5310a6d227c`; the optimized app
SHA-256 is `2e19caea6b2ad5245bfe1f3ec9bad7524ff5c5c7e35e0965133dfbe33e9a2114`.
The final full validation and optimized local build passed. Matching capture
components and local dynamic dependencies were checked again.
The follow-up build was opened at the owner’s testing request after confirming
no Redunar process remained. Its fresh process executable matched that digest;
no package was installed. Launch evidence is retained separately from fixture
acceptance in `local-launch-size-motion-process.json`.

## Approved disclosure and spinner integration

After reviewing the live preview, the owner approved the shared unfolding of
Filters, detected Steam launch options, Library More and clip action menus,
plus the simple export-button spinner. These now run in the production UI.
The spinner replaces the idle icon while busy, prevents duplicate submission,
preserves Cancel, and stops rotating under reduced motion. Progress retains
source thumbnail, range and original size in the compact dialog. Retrying
replaces the previous failure heading with the active operation heading.

Final review also corrected three nearby presentation cases: decimal MB is
consistent between clip cards and export, the first arrival into an empty
collection fades, and a failed thumbnail retains its placeholder after filter
redraw. Original recording visibility after a save was already fixed in base
commit `5564ca6`; this branch preserves that behavior rather than duplicating it.

The preceding integrated acceptance was recorded separately in
`.redunar-build/reports/clip-compression/acceptance-approved-ui.json`.
Earlier hashes and passes above identify their own intermediate builds.
Remaining owner acceptance is a real-recording check of whole and trimmed
exports, size limits and retained audio. No installation, live-game run,
package qualification, commit, pull request upload or merge is implied by
the local build checks.

That approved UI runtime source manifest SHA-256 was
`709d58cd6bee0d3a02c37fb456182994ad98de0194e1de0ac821c8e1ca192838`; the optimized app
SHA-256 is `151e903a7d63127d67c8b426d519969fe6c824a19c8252af4e776967a12ba598`.
Full validation and the local optimized build passed. Production WebKit
passed the export checks, 29 motion checks, 15 inventory checks and 152 workspace
checks. The final narrow progress and disclosure screenshots were inspected.
Capture-component digests and local dynamic dependencies match this build.
This newest build has not been launched; the earlier manual-launch evidence
above belongs to its earlier binary.

The final pre-PR upstream check found `origin/main` at `3baa431`, adding only
roadmap/security documentation after the initial `825012d` baseline. The task
branch was advanced to that base without conflicts or runtime changes. The
roadmap now includes the implemented compression/export behavior. Final
integrated checks are recorded in the current approved-UI acceptance report;
the intermediate hashes above remain historical evidence for their builds.
The final cleanup review also established temporary-directory ownership before
creating the output file. An early file-creation failure now removes that job's
private directory; a focused regression forces this failure and checks cleanup.

## Source-relative compression controls

The October 18 design refinement replaces the fixed slider range with a ceiling
bounded by the measured whole recording, or a duration-proportional selection
estimate explicitly marked Selection ≈. Both retain the native 500 MB ceiling,
default to half the baseline within supported bounds, and show the baseline
leading to Under the selected allowance. Whole-clip allowances that already fit
prompt for a smaller value; missing or sub-0.1 MB baselines disable compression
without blocking ordinary trimmed export. Retry preserves full-source duration
so the selection estimate cannot silently become the whole recording's size.

Fresh `tools/validate.sh full`, frontend tests/build, optimized native build and
production WebKit Replay checks passed after this refinement. Nine additional
private WebKit preview checks passed, including source-relative bounds, typed
over-limit values, tiny clips and ordinary export. The production normal/narrow
whole-clip screenshots and the preview's expanded selection comparison were
visually inspected. Adjacent capture-component digests match the local build.

Current evidence is in
`.redunar-build/reports/clip-compression/acceptance-size-reduction.json`.
The runtime source manifest SHA-256 is
`7a47c026371a6595fe9b30bd8c32e13610bda36cc167817a85160cacf12223b2`;
the optimized app SHA-256 is
`ebaca72a29e848af68efc51382a827a370f09644389b9fa5aee6f9ac760e1234`.
This build has not been launched or installed. Real-recording acceptance and
release/package qualification remain separate. No dependency, asset, license,
version or persisted-format change was introduced by this refinement; no files
were deleted. Earlier hashes above identify only their dated builds.

After PRs #8, #9 and #10 merged, the task branch advanced without conflicts to
`95b166a`. Those upstream changes add download-statistics tooling and branding/
workflow documentation; the runtime manifest above is unchanged. The final
integrated validation is recorded separately from the pre-integration checks.
