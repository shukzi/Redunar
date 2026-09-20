# Verification guide

Current Tauri and shared-backend checks, reviewed September 20, 2026. Run commands
from the repository root unless explicitly stated otherwise. Use existing offline
dependencies. Root Cargo commands do **not** include the separate Tauri workspace.

## Choose checks for the change

| Change | Required evidence |
| --- | --- |
| Documentation only | Source/decision consistency, local links, and command paths; no app rebuild required |
| Frontend logic | Relevant Node tests and production build; affected UI visually inspected |
| Native commands/lifecycle | Focused Tauri Rust tests, build, Clippy, and relevant bridge checks |
| Shared backend/protocol | Relevant root crate tests, formatting/Clippy, and affected native caller tests |
| Capture/overlay runtime | Fixtures first, then explicitly scoped native/GPU/game validation where needed |
| Packaging/install behavior | Full Tauri release/staging checks; installation only in an authorized workflow |

Do not repeat an expensive full gate without new changes or a reason. A bug fix
needs meaningful regression coverage of its failure, not tests that mirror the
implementation. Ordinary tests use isolated state, fake hardware, and synthetic
media. Never run destructive hardware tests automatically.

## Automated commands

Shared Rust workspace:

```sh
cargo fmt --all -- --check
cargo test --offline --workspace
cargo clippy --offline --workspace --all-targets -- -D warnings
```

Use `-p <affected-crate>` for focused iterations. Tauri workspace and frontend:

```sh
cargo fmt --manifest-path output/tauri-redunar/src-tauri/Cargo.toml --all -- --check
cargo test --offline --manifest-path output/tauri-redunar/src-tauri/Cargo.toml --all-targets
cargo clippy --offline --manifest-path output/tauri-redunar/src-tauri/Cargo.toml --all-targets -- -D warnings
node --test output/tauri-redunar/tests/*.test.mjs
tools/build-linux-release.sh
```

The production release/package gate is:

```sh
tools/check-tauri-release.sh
```

It builds Tauri, runs native/Node/lint checks, validates desktop/AppStream metadata
without network access, checks the staging tree and privilege boundaries, and
builds/checks an RPM when `rpmbuild` is installed. It does not install it or run
the ignored hardware/game acceptance tests. Inspect the script when changing
release behavior; do not substitute root-only Cargo success for this gate.

Installer-only checks do not install packages or contact GitHub:

```sh
tools/test-installer.sh
tools/build-tauri-deb.sh target/install-assets
tools/build-tauri-arch-package.sh target/install-assets
tools/build-tauri-opensuse-rpm.sh target/install-assets
tools/build-tauri-portable.sh target/install-assets
tools/render-public-installer.sh example/redunar /absolute/output/install.sh packaging/release-signing-public.pem
REDUNAR_GITHUB_REPOSITORY=example/redunar ./install.sh --check
```

The installer suite covers Fedora, Ubuntu/Debian, Arch, and openSUSE plans plus
old-glibc, ARM, Alpine, NixOS, and immutable-Fedora refusal paths. It also uses
an isolated local download and fake package manager to verify signed-manifest and
checksum success, corrupted-asset rejection, and invalid-signature rejection
without changing the host. Package checks establish structure, dependency metadata,
and signature consistency, not runtime compatibility on those distributions.

## Built interface and media

After building, run the relevant WebKitGTK fixture checks:

```sh
xvfb-run -a python3 output/tauri-redunar/tests/workspace-webview.py
xvfb-run -a python3 output/tauri-redunar/tests/replay-webview.py
```

These use isolated frontend fixtures, not real user sessions. Inspect affected
screens at normal and compact widths and with keyboard input. Check empty,
loading, failed, inherited/custom, unavailable, and long-text states. Review
computed styles from the built page, not a CSS file that may be overridden.

For the real native playback command/stream/WebKit path:

```sh
cargo build --release --offline --manifest-path output/tauri-redunar/src-tauri/Cargo.toml --example replay_webview_probe
xvfb-run -a output/tauri-redunar/src-tauri/target/release/examples/replay_webview_probe
```

The default probe generates isolated H.264/Opus media and requires a decoded frame
and filmstrip samples. An optional clip argument uses a private copy of that
recording. Do not log private stream tokens or modify source clips. Fixture
success does not establish real-game performance or every distribution's codecs.

## Retained engineering diagnostics

The following examples are intentional engineering tools. Keep them unless their
capability has a documented replacement. They are outside the ordinary automated
gate: select one for a specific question, record the build and host, and interpret
only the property it checks.

Synthetic and isolated diagnostics are safe for routine development:

```sh
cargo run --release --offline -p redunar-daemon --example replay_foundation_self_test
cargo run --release --offline -p redunar-daemon --example replay_profile
cargo run --release --offline -p redunar-daemon --example replay_store_profile
cargo build --release --offline --manifest-path output/tauri-redunar/src-tauri/Cargo.toml --example desktop_controls_probe
dbus-run-session -- xvfb-run -a output/tauri-redunar/src-tauri/target/release/examples/desktop_controls_probe
```

`replay_foundation_self_test` uses temporary state and deliberately reports that
hardware encoding and decoded playback are unverified. `replay_profile` measures
bounded in-memory ring insertion with synthetic packets. `replay_store_profile`
writes and removes one synthetic 16 MiB container under the temporary directory.
`desktop_controls_probe` uses an isolated D-Bus/Xvfb session, fixture preferences,
and a pipe-only shortcut helper; it does not open input devices or change host
settings.

These live probes require an explicit, scoped hardware run:

| Probe | Command and scope |
| --- | --- |
| PipeWire game audio | `cargo run --release --offline -p redunar-capture-audio --example game_audio_live_probe` starts a temporary silent `pw-cat` playback node, captures ten Opus packets, and terminates the child. It checks owned-node discovery/capture only. |
| PipeWire output fallback | `cargo run --release --offline -p redunar-capture-audio --example output_audio_live_probe` resolves the actual default output, starts temporary silent playback on that exact sink, captures ten Opus packets from its monitor, and terminates both children. It does not capture microphone input or retain audio. |
| Vulkan capture | Build `redunar-capture-vulkan`, then run `cargo run --release --offline -p redunar-daemon --example capture_probe -- /usr/bin/vkcube`. It launches the absolute target through the private layer and cleans isolated state. `REDUNAR_CAPTURE_PROBE_OVERLAY=1` checks rendered overlay submission; `REDUNAR_CAPTURE_PROBE_REPLAY_CANDIDATE=1` requests source eligibility; `REDUNAR_CAPTURE_PROBE_REQUIRE_REPLAY_CANDIDATE=1` makes missing eligibility fail; `REDUNAR_CAPTURE_PROBE_REPLAY_ENCODE=1` performs a real Vulkan Video encode. WSI, frame count, dimensions, FPS, metrics, preset, corner, opacity, scale, and timeout have bounded `REDUNAR_CAPTURE_PROBE_*` overrides in the example source. |
| KMS discovery/planning | `cargo run --release --offline -p redunar-capture-kms --example kms_replay_probe` enumerates local DRM outputs and constructs 60 FPS plans. It does not capture or encode frames. |
| KMS DRM read | `cargo run --release --offline -p redunar-capture-kms --example kms_drm_read_probe -- 1 DP-2` opens the selected card/connector read path and reports its active dimensions. Replace the example arguments with the intended output. |
| KMS encoder setup | `cargo run --release --offline -p redunar-capture-vulkan --example kms_encoder_probe` creates the production KMS Vulkan Video encoder at its fixed 2560x1440@60 diagnostic request. |
| KMS live replay | `cargo run --release --offline -p redunar-daemon --example kms_replay_live_probe -- 8` performs local KMS capture and hardware encode for the requested frame count. |

The KMS/DRM examples are retained exploratory diagnostics, not the production
per-game Replay path and not evidence of supported desktop capture. The
`redunar-platform` `prepare_capture_layer` example is a low-level manifest helper
for a supplied session directory and layer library; normal sessions prepare this
through the service and should not need the example directly.

## Regression areas

- **Profiles:** fresh/previous-version state, inheritance, stale revisions, unsaved drafts
  across games, partial failures, unrelated preference saves, and restart.
- **Visibility:** start a supported game with metrics hidden, show/hide repeatedly,
  and preserve runtime availability. A recording-settings lock must not block
  visibility-only changes; custom per-game values override inherited global ones.
- **Library:** bounded import, duplicates, ambiguous identity, missing executable,
  retained artwork after uninstall/cache removal, separate poster/banner roles.
- **Replay:** rapid selection/cancellation, rail scroll, native temporary-file
  exhaustion/errors, decode/seek deadlines, resize, save/export failure, source
  preservation, deletion boundaries, and final empty state.
- **Menu:** dedicated native document, transparent exterior, preview isolation,
  focus/Escape/outside-click, hidden main window, and close without app shutdown.
- **Shortcuts/tray:** empty/menu-only/save-only bindings, duplicates, persisted
  clearing, hidden-window dispatch, no disabled tray icon, live preference changes,
  missing providers/access, and recovery of a hidden window after tray loss.
- **History:** raw extrema/spikes, null gaps, real timestamps, units, axis/trace
  alignment, both CPU/GPU series, nearest-sample selection, previous-version and partial data.
- **Lifecycle:** direct/forwarded launch, duplicate instance, launch lock after End,
  natural exit, one history write, cleanup, unsupported capability, and restart.
- **Recovery:** fixture-only exact restoration, ownership conflicts, partial
  application, and crashes. The Tauri application must keep privileged actions
  outside the webview.

The [audio behavior](REPLAY.md#audio-behavior-and-output-fallback) needs separate
owned-node and intended output-fallback checks. The latter can include other apps.
Media-tool tests cover executable discovery, provider-neutral encoder checks,
cached success/retried misses, missing codecs, timeouts, and bounded probe output.
The RPM gate checks file/capability requirements without pinning a multimedia
package name or declaring replacement. Remaining release work is in ROADMAP.

## Installed app and owner-controlled runs

```sh
tools/check-installed-tauri-runtime.sh
```

This read-only check compares installed binary/sidecars with the release build.
After installing an update, fully quit Redunar, including its tray process, then
reopen it. A new RPM file does not replace an already-running executable. Record
build/version, host, test scope, and failures; do not hardcode old passing counts
as acceptance criteria.

Use [REAL-GAME-TESTING.md](REAL-GAME-TESTING.md) for the small agreed game matrix
and targeted Vulkan/session/replay runners. Do not run a blanket `--ignored`
test suite: select explicit hardware tests and understand their effects.

Old recipes and dated results are not part of the active Tauri release checklist.
