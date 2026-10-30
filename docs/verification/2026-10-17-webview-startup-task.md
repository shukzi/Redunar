# Webview settings startup crash

- Problem: the local release panics in `webview_settings.rs` because WebKit's
  `settings` GObject property is not readable. Prior Python fixtures used the
  supported getter and did not execute this Rust production path.
- Branch: `feat/workspace-motion`, base `0b71a05`; repair the current work without
  disturbing other local changes.
- Acceptance: production configuration obtains settings through the supported
  typed getter, disables easing for that view only, remains repeatable, and does
  not panic. Keep all prior performance, motion and thumbnail behavior.
- Ownership: main-window native setup and `webview_settings.rs`. No service,
  recording, persisted-data, profile, permission or cleanup changes.
- Reuse the already locked MIT `webkit2gtk` 2.0.2 binding (already a production
  transitive dependency with retained notices); add its direct Linux dependency
  edge without downloading packages or changing versions.
- Regression: run the real production function from a minimal Tauri probe in
  private D-Bus/Xvfb, with blank incognito views and temporary XDG state. Prove
  the old path fails, then verify no panic, disabled easing, repeatability and
  isolation from another view. No backend, games, hotkeys or tray workers.
- Checks: documented full validation, targeted native probe, production UI wheel
  and performance regressions, and a local release build. No installed-app launch,
  installation, restart, real hardware session, network or GitHub writes.
- Review: typed API ownership, missing settings, startup dispatch errors,
  dependency/license compatibility and exact native runtime coverage.
