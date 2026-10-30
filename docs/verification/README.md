# Verification evidence

- [October 29 NVIDIA and desktop integration](2026-10-29-nvidia-integration.md):
  external-memory synchronization/recovery, bounded import reuse, automatic
  hardware admission, desktop update retention and exact-package acceptance.

- [October 24 Tauri dependency security repair](2026-10-24-dependency-security.md):
  patched native and frontend dependencies, the GTK3-compatible GLib backport,
  optimized iterator regression and remaining build-time maintenance debt.

- [October 19 sparse-frame export duration](2026-10-19-export-duration.md):
  reproduced timestamp-shift inflation, retained duration guard and generated
  MP4/MKV, multi-audio and two-pass regression coverage.

- [October 19 native Wayland startup](2026-10-19-wayland-startup.md): shared
  desktop/Steam backend preference, real display-connection fallback and
  override checks, preserved child environment and lifecycle regressions.

- [October 19 stable Tauri comparison](2026-10-19-tauri-stable.md): released
  dependency update, matched native paused/playing rendering samples, retained
  fallback and no demonstrated full-refresh improvement from the update alone.

- [October 18 clip compression](2026-10-18-clip-compression.md): Size first
  whole-clip compression, optional trimmed-export foldout, MP4/MKV and verified
  size ceilings; offline tests, WebKit inspection and an uninstalled local build.

- [October 17 remaining release hygiene review](2026-10-17-release-hygiene.md):
  align Tauri version metadata, reject stale release inputs, correct native
  package/tray/update documentation and retire an unused X11 fixture helper.

- [October 17 original native scrolling restored](2026-10-17-native-scroll-restore.md):
  owner-requested removal of custom wheel animation and restored WebKit easing;
  memory-row stability and other performance improvements retained.

- [October 17 browser scroll frame timing](2026-10-17-browser-scroll.md):
  local Helium/native WebKit comparison exposes a per-notch repeated frame;
  fix retarget clock anchoring while retaining the existing finish deadline.

- [October 17 continuous mouse-wheel motion](2026-10-17-wheel-continuity.md):
  preserve speed between notches, ramp gently from rest and avoid prolonged
  boundary animation; replaces repeated ease-out restarts.

- [October 17 softer wheel response](2026-10-17-wheel-softening.md):
  owner tuning to a gentler 150 ms curve, retaining bounded settling and native
  bypass paths after the first 110 ms treatment.

- [October 17 wheel response refinement](2026-10-17-wheel-refinement.md):
  short coarse-wheel easing after owner feedback that direct steps felt abrupt;
  supersedes the direct-only interaction policy in the earlier scroll report.

- [October 17 native settings startup repair](2026-10-17-webview-startup.md):
  supported Rust getter and a real Tauri regression for the owner-reproduced
  startup panic, superseding the earlier Python-only runtime acceptance.

- [October 17 Overview scroll stability](2026-10-17-overview-scroll.md):
  direct native wheel response without trailing easing, retained memory-reading
  line space through monitor loss/recovery, and isolated input/geometry checks.

- [October 17 History directions and thumbnail reuse](2026-10-17-history-and-thumbnails.md):
  partial sensor coverage transitions and bounded persistent thumbnail caching
  with native permanent-deletion cleanup.

- [October 16 workspace interaction motion](2026-10-16-workspace-motion.md):
  bounded dialog/dropdown/draft-bar entry, reduced motion, immediate cancellation
  and retained refresh performance regressions.

- [October 16 desktop refresh performance](2026-10-16-ui-refresh.md):
  unchanged-status DOM retention, startup update isolation, lazy microphone
  discovery, full validation and production WebKit regressions.

- [October 15 workspace refinements](2026-10-15-workspace-refinements.md):
  persistent clip trash, named exports, draft retention, audio status, updater
  restart guards and integrated keyboard reconnect checks.

- [October 14 keyboard reconnect recovery](2026-10-14-keyboard-reconnect.md):
  automatic same-user rediscovery, stale-input/menu cleanup, private fixtures,
  production UI inspection and a local Arch test package.

- [October 13 Steam discovery review and Rust 1.98 validation](2026-10-13-library-discovery.md):
  reviewed app-ID matching and import feedback, isolated production UI checks,
  two behavior-preserving Clippy repairs and full offline validation.

- [October 10 Steam X11 launch integration](2026-10-10-steam-x11.md):
  contributor update, selective preload cleanup, real-wrapper fixtures and
  offline compatibility/package checks; installed-game acceptance remains separate.

- [October 9 Replay save layout and NVIDIA extension discovery](2026-10-09-replay-save-nvidia.md):
  stable duration controls, pending/error status and bounded complete Vulkan
  extension queries; successful NVIDIA encoding still needs hardware acceptance.

- [October 9 temporary tray and live-game window close](2026-10-09-steam-temporary-tray.md):
  revised startup and close policy, compositor-focus regression, current private
  fixtures and production build; installed-game acceptance remains separate.

- [October 9 initial Steam automatic exit](2026-10-09-steam-auto-exit.md):
  historical first implementation and installation, superseded after owner testing.

- [October 9 Replay menu pointer lifecycle](2026-10-09-replay-menu.md): removed
  mouse handles, stale input and view-only polling fixes; local packages built,
  owner-confirmed installed-game menu check on CachyOS.

- [October 8 ultrawide Replay and Intel temperature](2026-10-08-ultrawide.md):
  shared pixel-budget admission, Variable ceilings, package sensor discovery and
  current private-fixture/build evidence; hardware encoding remains unverified.

Add dated reports tied to an exact commit or source-manifest hash. Record commands,
environment, success/failure, excluded scope, and artifact hashes where applicable.
Keep private diagnostics and recordings outside Git and reference them cautiously.
Generated logs live in the ignored `.redunar-build/reports/` directory. A committed
summary should describe the evidence and limitations without dumping private logs.
Historical results in TESTING remain available; new long-form reports belong here.

- [October 8 X11 GLX lookup forwarding](2026-10-08-x11-glx.md): actual-sidecar
  fake-provider repair; reported ARC Raiders cause remains unconfirmed.

- [October 8 native tray task](2026-10-08-native-tray-task.md): direct
  StatusNotifier/DBusMenu client and combined release scope.

- [October 9 native tray and combined candidate](2026-10-09-native-tray.md):
  direct tray lifecycle/protocol tests, local DMS registration and combined checks.
