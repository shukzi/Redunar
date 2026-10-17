Native tray dependency notices
==============================

The current Linux tray is implemented in `src-tauri/src/tray_native/` using
GTK's existing GIO/GLib session-bus interfaces for StatusNotifierItem and
DBusMenu. The native Cargo manifest does not enable Tauri's `tray-icon` feature,
and the active tray does not require an AppIndicator provider or directly use
`libloading`.

The notices in this directory include GTK 0.18.2 (MIT), as well as earlier
tray-icon 0.24.2 and libappindicator 0.9.0 (MIT OR Apache-2.0), and libloading
0.7.4 (ISC) dependency coverage. Optional packages remain in the lockfile and
broader generated inventory; that does not establish that they are linked into
the current Linux tray. These files are retained pending an individual licensing
review, not a declaration of an active AppIndicator runtime dependency.

See the generated [dependency inventory](../../../../packaging/DEPENDENCY-LICENSES.md)
and [third-party notices](../../../../packaging/THIRD-PARTY-NOTICES.md) for
distribution coverage. `tools/stage-tauri-package.sh` owns the packaged notices.
Do not remove a notice solely because its package is absent from one active
feature graph.
