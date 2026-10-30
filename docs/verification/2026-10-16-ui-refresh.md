# Desktop refresh verification — October 16, 2026

Branch `fix/ui-refresh-performance`, based on
`d1a46d144759786f1ea23cf332657c740b9859e9`, with local working-tree changes.
Scope and acceptance are in the [task brief](2026-10-16-ui-refresh-task.md).

## Behavior and ownership

- Capture status compares its generated markup with the last generated string,
  retaining unchanged DOM through polling despite browser SVG normalization.
- Startup update completion refreshes only the update card/sidebar badge. It
  retains the workspace, Replay player and preparation, search and focus.
- Microphone input discovery is deferred until its settings tab is opened.
  Pending visits share one read; completion changes only input choices and
  feedback. Saved source identity, recording/appearance drafts and unrelated
  controls survive success, failure and explicit retry. Failed discovery clears
  stale choices and retains an unavailable saved source rather than substituting
  the default input.

The shared service still owns microphone preferences, session locks and native
discovery. No persisted format, dependency, renderer default, playback
normalization, media-worker limit or thumbnail scroll delay changed. See
[PERFORMANCE](../../PERFORMANCE.md#desktop-refresh-work) for the current contract.

## Current checks

CachyOS x86_64, Rust 1.97.0, Node 26.11.1 and WebKitGTK 2.54.1. All ordinary
runtime tests use private state/fake hardware; UI checks use native-command
fixtures and generated test video in Xvfb.

Final full-validation log:
`.redunar-build/reports/20261016T205834Z-full.Nwex7Y.log`.
Its source-manifest hash is
`2bbcbabefffa7fd0e6fe624b9a808f78bd2933b152a7d48a7413188003892aff`.
Initial/final manifests match. All manifested files still matched when the UI
checks finished; only this evidence and its index links were added afterward.

| Check | Result |
| --- | --- |
| `tools/validate.sh full` | Passed: both Cargo workspaces checked/tested, strict Clippy and formatting, 818 Rust tests passed / 12 ignored, 104 Node tests and 13 tooling tests passed, production Vite build passed |
| `xvfb-run -a python3 output/tauri-redunar/tests/performance-webview.py` | 10 checks passed: zero capture-summary mutations across unchanged polling, real failure transition, delayed update preserving player/filter/focus, lazy/coalesced discovery, retained controls/drafts, failure and retry |
| `xvfb-run -a python3 output/tauri-redunar/tests/workspace-webview.py` | 149 checks passed, including saved/unavailable microphone sources, failed preference writes, discovery retry, keyboard controls and narrow layouts |
| `xvfb-run -a python3 output/tauri-redunar/tests/replay-webview.py` | Passed: rail/document scroll, focus, selection races, thumbnail updates, readiness, eight decoded filmstrip frames, keyboard trimming and export boundaries |
| `xvfb-run -a python3 output/tauri-redunar/tests/refinements-webview.py` | 24 checks passed, including clip trash/restore, retained drafts, updater errors and runtime audio status |
| Visual inspection | Production Overview and Replay plus microphone loading, ready, error and narrow screenshots inspected |

UI logs use `.redunar-build/reports/ui-refresh-*.log`; screenshots/check lists
are under `workspace-ui/` and `replay-ui/` in the same reports directory.
Xvfb reports its expected missing-DRI3 warnings; this is not accelerated-desktop
performance qualification.

## Review, recovery and limits

A separate local review of the final diff checked production callers, pending
request ownership, failed reads/writes, navigation, session locks, saved-source
compatibility and unchanged native cleanup. No independent reviewer was used.
The change is reversible in the frontend and requires no data migration.

The automated verification did not install or restart the installed app, publish
externally, capture a real microphone, run a game or compare renderers. The WebKitGTK DMA-BUF
crash workaround remains enabled. These tests establish removed redundant work
and retained behavior; they do not establish a scrolling FPS improvement on the
owner's compositor. Full-clip playback preparation remains a possible opening
delay and retains its existing compatibility handling.

## Subsequent manual acceptance

On October 16, the owner tested the local native build without installation,
reported a noticeable improvement in scrolling/loading, and confirmed that the
app worked after being asked to check recording, Instant Replay and the in-game
overlay. This is owner-reported acceptance, not an instrumented frame-time
measurement or independent verification of every hardware path. The owner then
requested integration of the tested changes.
