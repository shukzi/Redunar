# Native settings startup repair — October 17, 2026

Branch `feat/workspace-motion`, base `0b71a05`; see the
[task brief](2026-10-17-webview-startup-task.md).

The owner's local release crashed before opening because the native configuration
read `WebKitWebView.settings` through generic GObject property access. The property
is not readable that way. Existing Python fixtures called `get_settings()`, so
their pass did not cover this failing production Rust call. Compilation and
Rust tests likewise did not execute window setup. This supersedes the earlier
[native scroll acceptance](2026-10-17-overview-scroll.md).

`webview_settings.rs` now uses `WebViewExt::settings()` and the typed
`SettingsExt::set_enable_smooth_scrolling(false)`. Missing settings leave the
native default in place and report a static operation-log message; dispatch
failures retain the existing nonfatal handling. The getter returns the native
reference-counted settings object; no raw property reflection is used.

The direct Linux dependency edge reuses exactly the existing locked `webkit2gtk`
2.0.2 binding. Cargo.lock changes only the Redunar dependency list, without
new packages, version changes or downloads. Its MIT metadata/license text were
already retained in `packaging/DEPENDENCY-LICENSES.md`; the owning notices now
explain the direct use. Renderer flags, scrolling policy, UI motion, services,
recordings, persisted profiles and package installation behavior are unchanged.

## Regression and limits

`examples/webview_settings_probe.rs` includes and calls the production function
through a real Tauri webview in private D-Bus/Xvfb. It uses blank, hidden,
incognito views, a distinct app identifier, temporary XDG directories with mode
0700, and no backend/tray/hotkey/game workers. A deadline prevents hangs.

The old production function failed this probe with the owner's exact panic
(`webview-startup-before.log`, exit 101). Temporary state was cleaned on unwind.
The corrected probe must confirm easing is disabled after repeated configuration
and remains scoped to the intended view. This closes the Rust-versus-Python
coverage gap. Desktop touchpad feel and game recording remain separate owner
acceptance; no installation or restart is performed.

## Current checks and review

- Old-function native reproduction: `webview-startup-before.log`, exact unreadable
  property panic and exit 101. Corrected debug native probe:
  `webview-startup-after.log`, pass.
- Production local build: `webview-startup-release-build.log`, pass, including
  the native executable and existing sidecars/helpers. Executable fingerprint:
  `webview-startup-runtime.sha256`. This is a local build, not an installed app.
- The optimized native probe also passes after compiling the same production
  module in release mode: `webview-startup-release-probe-build.log` and
  `webview-startup-release-probe.log`.
- Production UI regressions: 3 real private-display wheel checks and 10 existing
  performance checks pass (`webview-startup-wheel.log`,
  `webview-startup-performance.log`). No frontend implementation or motion code
  changed in this repair, so the unrelated full UI fixture sweep is not repeated.
- Locked dependency audit: all 442 package identities, versions, sources and
  checksums match the previous lockfile. Only Redunar's direct dependency list
  adds the already present binding; existing license inventory covers it.
- Final full validation output: `webview-startup-full-console.log`, with the
  wrapper's dated log and before/after source manifests identifying the exact
  final tree. Both workspaces, strict Clippy, frontend and tooling are included.
- Separate review: production entry calls the same function imported by the
  native probe; the getter and setter use supported typed APIs. Missing settings
  and dispatch failure retain nonfatal static feedback. No runtime services,
  renderer, persisted state or other-view settings are changed. The fixture
  cleanup guard takes ownership only after directory creation succeeds and
  cleans its own temporary state on normal exit, unwind or deadline.

All logs are under `.redunar-build/reports/`. `git diff --check` and the new
documentation links pass. No installed runtime or game/compositor acceptance
is claimed by these isolated checks.
