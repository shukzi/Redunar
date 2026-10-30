# Native tray and combined release task — October 8, 2026

- Request: replace Redunar's AppIndicator backend with direct StatusNotifier,
  combine the prepared ultrawide/Intel temperature/GLX repairs, qualify and ship
  a new version if checks succeed.
- Branch/worktree: feat/native-tray-release, based on a6ebea6 (v0.1.14).
- Owners: native tray facade, private D-Bus backend and menu; app shutdown and
  Steam background fallback; exact dependency lock/license inventory; release
  metadata and integrated existing fixes.
- Acceptance: no AppIndicator load/dependency; typed SNI properties/icon and
  DBusMenu Open/Quit; one registration per enabled instance; disabled startup
  exports nothing; missing/denied/replaced watcher or absent host cannot hide
  the app; watcher recovery re-registers; stale async replies cannot grant hiding.
- Checks: private D-Bus fake watcher/host protocol and lifecycle tests, malformed
  requests, repeated toggles, registration rejection/loss/recovery, cleanup;
  integrated root/Tauri full checks, production UI visual fixtures, sidecar
  checks, exact compatibility/package/release candidate qualification.
- No privileged changes, extra service, desktop detection, GNOME extension,
  installation, live game/hardware capture, or telemetry. AppIndicator dependency
  workaround is superseded; retain CachyOS installer fixture coverage.
- Existing preference/wire formats remain compatible. GLX report remains an
  unconfirmed ARC Raiders repair; NVIDIA software checks are not GPU acceptance.
- GitHub readable text requires exact owner review before upload. Prepare all
  implementation, evidence, packages and final wording before that approval.
