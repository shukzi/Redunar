# Stable Tauri rendering comparison, October 19, 2026

The dependency update uses released Tauri 2.12.4 and JavaScript API 2.12.4,
tauri-build 2.7.3, tauri-runtime/runtime-wry 2.12.2, Wry 0.57.0 and Tao 0.37.1.
The previous native graph used Tauri 2.11.5, Wry 0.55.1 and Tao 0.35.3, with
JavaScript API 2.11.1. Minimum manifest versions and both lockfiles advance
together; the dependency-license inventory is regenerated. Upstream API license
texts are unchanged. [Official stable release](https://github.com/tauri-apps/tauri/releases/tag/tauri-v2.12.4).

The comparison retains the accepted UI-polish changes based on merged
`18649ac640a7797bd94cef0cd85209d04cec084d`, also the refreshed remote main at
the time of testing. It does not include the separate GTK 4 experiment. Native
commands, services, persisted profiles, cleanup and production renderer policy
are unchanged. Explicit renderer overrides remain honored; the default DMA-BUF
workaround is held at `1` for both builds. Native smooth scrolling is unchanged.

## Matched native fixture

`rendering_probe` embeds each version's built UI in an actual Tauri window.
The same diagnostic source and generated 40-clip inventory are used for both
versions. A private native handler supplies fixture responses over Tauri's real
IPC; it registers no production backend commands. Clips, thumbnails and playback
are generated locally. No real game, recording, input device or profile is used.

Four phases cover paused scrolling, paused dropdowns, playing scrolling and
playing dropdowns. The rail is warmed first. All real-desktop phases retained
focus; a private Xvfb run separately established loading and IPC behavior.
No compiler or validation workload ran during the desktop timing comparison.

| Observed desktop proxy | Previous graph | Updated graph |
| --- | --- | --- |
| X11 GTK paint median, all phases | 16.667 ms | 16.667 ms |
| X11 continuous scroll rAF, paused / playing | 61.80 / 60.04 per second | 61.40 / 60.00 per second |
| Wayland GTK paint median, paused scroll | 11.762 ms | 11.762 ms |
| Wayland continuous scroll rAF, paused / playing | 101.88 / 87.94 per second | 101.81 / 87.99 per second |
| Warm dropdown event-to-DOM scripting | Mostly 1–3 ms | Mostly 2–3 ms |

These samples show no meaningful smoothness improvement from the dependency
upgrade alone. Neither graph demonstrates full refresh-rate presentation.
RequestAnimationFrame and GTK after-paint are diagnostic proxies; scripted rail
movement and dropdown clicks do not measure physical input latency or screen
scanout. Results from one desktop do not qualify every GPU or compositor.
Display settings were not changed.

Raw reports, logs, generated media, dependency versions and exact source/binary
manifests are retained under `.redunar-build/reports/tauri-stable/`. Qualification
also requires the full offline gate, native settings, tray/lifecycle and generated
playback probes, a production native build and the relevant package checks.
Those results belong to the exact local candidate report, not to an older pass.

The October 19 local candidate passed the full offline gate (both Cargo
workspaces, strict Clippy, the frontend tests and tool fixtures), the native
settings and generated playback probes, all 15 startup/window-lifecycle cases,
tray/shortcut cleanup, 45 UI-polish checks and the license check. The native
production build and native Arch package were produced without installation.
The competing-loader Vulkan binding guard and package payload/hash checks are
recorded with the candidate. These are local validation results; supported
distribution release qualification and real-game acceptance remain separate.

## Fallback

The existing executable and Arch package are retained unchanged. Their SHA-256
values were verified before this work:

- Executable: `ef12f40934b7d2c67f2c4bcad49e8166c0aa71763ca6f04462f7b683d5db2302`.
- Package: `c31f5366583ca9161ec5857dc98a81d42d6bc419322152e6231b41b5c678d862`.

The update is a local task branch. Building and comparing it does not install,
restart or publish the production app. GTK 4 work remains separate.

The subsequent [native Wayland startup change](2026-10-19-wayland-startup.md)
adds an explicit desktop/Steam backend preference. The unchanged-policy
comparison above describes the earlier matched dependency experiment.
