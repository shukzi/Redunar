# Redunar native app

The active Tauri application links one app-lifetime
`RedunarService::for_tauri()` to the shared Rust backend. The native host owns
commands, supervision, media workers, tray, and windows.

Current guides, reconciled September 13, 2026:

- [Architecture and source ownership](../../ARCHITECTURE.md)
- [Approved design and interaction rules](../../DESIGN.md)
- [Replay, media, shortcuts, and audio limitations](../../REPLAY.md)
- [Testing and package checks](../../TESTING.md)
- [Current release gates](RELEASE-CANDIDATE.md)
- [History calculation contract](HISTORY-CALCULATIONS.md)

## Build and inspect

All commands below run from the **repository root**:

```sh
python3 output/tauri-redunar/build-native.py
node --test output/tauri-redunar/tests/*.test.mjs
cargo test --offline --manifest-path output/tauri-redunar/src-tauri/Cargo.toml --all-targets
cargo clippy --offline --manifest-path output/tauri-redunar/src-tauri/Cargo.toml --all-targets -- -D warnings
tools/run-tauri-release-local.sh
```

The runner starts the real local app with matching adjacent capture components
and same-user shortcut helper. It does not install files into `/usr`; ordinary
tests use isolated state instead. Do not run it against an already-owned live
coordinator and infer fixture safety from a checkout launch.

For the complete automated build/package gate, run
`tools/check-tauri-release.sh`. To create the stable local package separately,
run `tools/build-local-tauri-rpm.sh target/packages`. Packaging details and
runtime dependencies are owned by [packaging/README.md](../../packaging/README.md).

The installed executable is `/usr/bin/redunar-tauri`; the package is
`redunar-app`. A built RPM is not installed automatically. Verify installed
components with `tools/check-installed-tauri-runtime.sh` and fully quit/reopen
Redunar after an upgrade so its embedded UI is reloaded.

The in-app update boundary now checks signed release metadata, selects the
matching supported Linux package, downloads it with a size limit, verifies its
checksum, retains it in a private cache, and exposes a native handoff to the
desktop package installer. Settings exposes manual checking and the default-on
automatic-check preference. Native code owns the handoff and restart boundary;
the webview must not install packages or run arbitrary commands. Manual package
installation and rerunning the signed installer remain supported, with user
data preserved across updates.

Release builds produced by `build-native.py` embed the HTTPS GitHub release
channel at `https://github.com/shukzi/Redunar/releases/latest/download`.
`REDUNAR_UPDATE_SOURCE_URL` can override it for an isolated test feed.
Development builds without either value deliberately report that signed update
checking is not configured and do not contact a release service.

`python3 output/tauri-redunar/install-desktop.py` is the optional local desktop
registration helper. It requires installed components matching the current build
and writes the user launcher; it is not a package installer. Do not invoke it
as part of a read-only review.

## Runtime/data boundaries

- Library, profiles, history, clips, and runtime status come from native data.
  Empty or failed reads do not fall back to sample games or fabricated graphs.
- Profile drafts are distinct from saved values. Saves preserve inheritance,
  detect stale changes, and retain unrelated pending edits.
- Supported launches prepare the metrics runtime even while hidden and request
  replay automatically. Retired controls do not reappear in the active UI.
- Overlay visibility saves update the prepared running game with inheritance
  respected. Recording locks must not block a visibility-only save.
- Native supervision and shortcuts continue with the webview hidden. End session
  releases resources without killing the game and retains its launch lock until
  the owned process exits. Quit/exit must clean up once.
- The Replay menu shortcut renders the menu inside the captured game through the
  Vulkan layer; there is no desktop menu window. The in-app preview dialog is
  only for configuration checks and never loads the desktop shell as backdrop.
- The main window uses Redunar's compact titlebar and native window commands so
  its size and controls remain consistent across supported X11 and Wayland
  desktops without selecting a compositor or display backend.
- Close to tray immediately controls icon visibility. Missing tray support must
  not strand an invisible window. Empty shortcut configurations remain valid.
- The service does not expose arbitrary privileged commands. The package
  contains only the current runtime components.

## Design promotion and assets

The native files are the source of truth for the shipped Tauri interface.
Preserve the native command/data boundary; never promote memory-only fixtures or
success simulations.

`ui/index.html` declares the effective stylesheet order. Inspect all relevant
scoped overrides when correcting a visual issue. Keep Noto font notices and
Comet R assets. Locally cached Steam posters/banners are optional runtime user
data, not bundled game assets or a cloud dependency.

## Verification records

Use the Node/Rust/WebKit fixtures and native replay probe in TESTING before
owner-controlled [real-game acceptance](REAL-GAME-ACCEPTANCE.md). A browser visual
pass does not verify native codecs, GPU performance, or compositor behavior.

Only current Tauri behavior and its documented release checks are requirements.
