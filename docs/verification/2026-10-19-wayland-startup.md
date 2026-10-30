# Native Wayland startup, October 19, 2026

This follows the [stable Tauri comparison](2026-10-19-tauri-stable.md) on the
UI-polish task tree based on merged `18649ac640a7797bd94cef0cd85209d04cec084d`.
The owner reported smooth scrolling after reopening the same native binary on
Wayland instead of X11. That is subjective desktop acceptance, not proof of a
particular presentation rate or of behavior on every system.

Production startup now applies a process-local GDK backend preference before
GTK initialization or service threads. A Wayland session prefers `wayland,x11`;
an unavailable Wayland connection falls back to X11. An explicit X11 session
stays on X11. A launcher without a session type can use `WAYLAND_DISPLAY`.
An explicit `GDK_BACKEND` remains authoritative. Desktop and Steam background
startup use the same entry point. The environment inherited by games and
helpers, renderer workarounds, service ownership and persisted state are unchanged.
The complete policy is owned by [ARCHITECTURE.md](../../ARCHITECTURE.md).

## Actual display connections

`tools/check-display-backend.py` passed all twelve cases using private Xvfb,
headless Mutter and a private D-Bus session. Nine GTK cases verify the selected
display type for normal Wayland, missing session type, absent or unreachable
Wayland, X11, nested Wayland on X11, explicit overrides and reversed ordering.
Each also verifies that the inherited backend environment is unchanged.

Two Tauri Steam startup cases verify native Wayland and X11 fallback, hidden
startup, tray registration, automatic-owner eligibility, explicit Open and
normal cleanup. A foreground Tauri fixture loads the production UI over real
IPC and verifies native Wayland while exercising paused and playing clip
scrolling and dropdowns. Its media and command responses are generated fixtures.

The existing fifteen X11 background-startup, activation, input and cleanup
regressions also passed. Their X11 focus-anchor checks remain intact. They do
not establish foreground-game focus behavior on a Wayland compositor; the new
headless cases assert hidden startup and explicit Open separately.

## Qualification and limits

Exact source manifests, commands, logs and artifact hashes are retained under
`.redunar-build/reports/wayland-startup/`. Full offline validation, the native
production build and native Arch payload checks are recorded against the final
working-tree manifest there. Earlier passes do not qualify a later source tree.

No display settings, installed application, real game, recording or user profile
are changed by these fixtures. No fixed refresh rate is imposed. An X11
fallback preserves startup availability; it does not promise identical frame
pacing. A display connection fallback does not recover arbitrary compositor or
GPU failures after initialization. Existing owners keep their backend until a
normal restart. The prior UI-polish and stable-Tauri binaries and packages are
retained for recovery. Distribution release qualification and installed-game
acceptance remain separate.
