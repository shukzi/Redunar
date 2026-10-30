# Workspace motion verification — October 16, 2026

Branch `feat/workspace-motion`, based on
`0b71a05774bcc8557f0b0d0859c5827ee11df5f3`, with local working-tree changes.
See the [task brief](2026-10-16-workspace-motion-task.md).

## Behavior and ownership

Production `ui/index.html` loads `workspace-motion.css` after the existing
refinements. Desktop dialogs fade in over 140 ms without a transform, preserving
the positioning context of their fixed menus. Precision menus fade/move 2 px
over 100 ms. Save/Discard bars fade/rise 4 px over 140 ms on first appearance.
Draft-action presentation retains a global/per-game owner key across re-renders
so further edits, polling and settings tab changes do not replay entry.

Dismissal, hidden-state cleanup, focus and native actions stay immediate.
Effects use opacity/transform only, without fill retention, persistent layer
hints, dependencies, timers or frame callbacks. This applies to the three CSS
entry effects; the separately requested History morph is described below.
Reduced motion disables these effects.
Shared services retain persistence, inheritance and session/capture authority;
no native commands, playback preparation, renderer flags or data formats changed.

The owner-reported Library flicker came from full workspace rendering on every
Game settings/Launch matching click. These tabs now replace only their detail
body, retaining the Steam panel (including pending checks, expanded options and
text selection), search, artwork, focus and unsaved actions. Selecting the active
tab is a no-op. Per-game option edits, reset, discard and saves now use the same
scoped detail update and refresh catalog draft labels in place. Failed saves
retain their drafts and retry actions without replacing Steam evidence. Native evidence still refreshes on entry, window focus and retry;
existing panel-scoped request guards reject late results for a previous game.

Read-only comparison with the concurrently edited local website's
`professional-appearance/feature-demos.css` confirmed similar dialog/save-bar
opacity and small translation effects. Its History preview in `showcases.js`
instead interpolates SVG coordinates for 520 ms on metric selection. The owner
then explicitly requested that style in the app. A separate `history-motion.mjs`
interpolates compatible presentation geometry for the same duration with a
1,024-vertex budget including departing secondary traces. The original cutoff
skipped dense histories entirely; owner feedback prompted bounded transition
geometry using per-bucket extrema from both shapes at existing timestamps.
Every original point/attribute returns at completion or inspection. Incompatible
gaps and isolated readings remain immediate. Primary fills and GPU lines now
fade during the morph, including departure cleanup. Settled data is never
downsampled; no journal or measurement calculation changes.
Metric changes update only the chart region, retaining catalog, filters and
button focus. Inspection/scrolling, completion, navigation, hidden windows and
reduced motion settle exact destination geometry and cancel the single frame
chain. Rapid switches start from the current geometry. Entry/polling never start
the effect. No prototype files were edited. Per-frame SVG work has a real cost;
this fixture establishes bounds and cleanup rather than a zero-cost claim.

## Automated and visual acceptance

Environment: CachyOS x86_64, WebKitGTK 2.54.1, private command fixtures/generated
media in Xvfb, with the production DMA-BUF workaround. No live app is launched.

| Check | Result |
| --- | --- |
| Production Vite bundle | Passed |
| `motion-webview.py` | 18 passed: bounded entry, focus/keyboard selection, immediate cancellation, completed animations released, no restart across edits/tab renders/polling, failed-save retry, inherited drafts, narrow bounds and reduced motion |
| `library-tabs-webview.py` | 13 passed: pending-check identity, no duplicate native reads, command/selection/search/draft retention, focus and no repeated entry, error/retry/ambiguity, different games and late completion, option edits/reset/discard/save success and failure, narrow layout |
| `history-motion.test.mjs` | 6 passed: exact completion/area, rapid switching, cancellation/disconnection and gaps/isolated-data bypass and dense/rapid transitions restoring every point |
| `history-motion-webview.py` | 15 passed: real WebKit metric switching, exact inspection, reduced-motion/navigation cleanup and bounded 7,200-sample transitions, fill/GPU fades and missing-data behavior |
| `performance-webview.py` | 10 passed: unchanged-status DOM retention, delayed update preserving Replay/filter/focus, lazy/coalesced microphone discovery, retained drafts and retry |
| `workspace-webview.py` | 149 passed: settings/inheritance, native-action payloads, errors, keyboard focus, microphone states, normal/compact/narrow layouts |
| `replay-webview.py` | Passed: scroll/focus, selection races, readiness, eight real filmstrip samples, trimming and export boundaries |
| `refinements-webview.py` | 24 passed: trash/restore, retained drafts, save/restart dialogs and updater error recovery |
| `tools/validate.sh full` | Passed after corrections: both Cargo workspaces, formatting/strict Clippy, 818 Rust passed / 12 ignored, 110 Node tests, 13 tooling tests and production frontend build |

Final correction validation log:
`.redunar-build/reports/20261016T215452Z-full.aYollL.log`.
Tested source-manifest hash:
`5ce2135ad57cc1db6c54b2dca6a6eafa35f2bf0bc2876f63c62275213c36a7c4`.
Initial/final validation manifests match. Only this report's result summary was
completed afterward; implementation and tests match the tested manifest.
All seven WebKit fixtures passed again against that production bundle. The
prototype's authoring and served `showcases.js` also matched during comparison;
no website files were changed.
Fixture logs are `.redunar-build/reports/workspace-motion-*.log`; screenshots and
check lists are in `.redunar-build/reports/workspace-ui/` (Replay in `replay-ui/`).

Normal-motion screenshots inspect the dialog, dropdown and draft bar during
entry, including a narrow bar. The fixture pauses CSS animations only to capture
these states; production code has no such controls. Legacy geometry assertions
finish only the bar's entry before checking its settled bounds. Separate motion
checks cover interaction/cancellation and viewport bounds during entry.
Additional screenshots inspect retained Steam setup, narrow Library and the
History graph during its transition and after exact inspection settlement.

The CSS reduced-motion branch reloads its isolated document under its process-local
GTK preference, checks the real media query/computed styles, and restores the
preference on cleanup. This qualifies stylesheet behavior on document load,
not live desktop preference switching. The separate History fixture also checks
a live process-local preference change settles a running graph immediately.

## Review and limits

Local review follows the shared modal/Precision controls and draft-action callers:
pending/failure states, disabled actions, global/game drafts, navigation,
immediate removal, keyboard focus and reduced-motion precedence. Existing refresh
regressions retain the previous performance fix. No independent reviewer was used.
The Library pass follows entry/focus/retry and stale native completions. History
review follows the real native journal DTO/service reader, source samples and
all metric/inspection/navigation callers; service/persistence formats are unchanged.
The transition module restores exact markup on every exit, and dense transitions
keep their frame work bounded without dropping settled recorded points. Extrema/rapid-switch tests cover the full journal-sized case.

No app installation/restart, real-game test, renderer comparison, package
qualification or publication is part of this task. Xvfb's expected missing-DRI3
warning means these results do not measure compositor frame times or prove zero
animation cost on the owner's desktop. The next acceptance is an owner-run local
build. Recovery removes these frontend additions; no data migration is required.
