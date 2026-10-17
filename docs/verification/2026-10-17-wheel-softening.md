# Softer wheel response — October 17, 2026

Historical duration tuning; [continuous mouse-wheel motion](2026-10-17-wheel-continuity.md)
records the owner's next feedback and the current controller.

Branch `feat/workspace-motion`, base `0b71a05`; follow-up scope is in the
[wheel task brief](2026-10-17-wheel-refinement-task.md). The owner preferred the
[110 ms treatment](2026-10-17-wheel-refinement.md) but requested more smoothing.

Only the duration/curve in `ui/workspace-wheel.mjs` change: a 150 ms quadratic
ease-out replaces 110 ms cubic easing. The initial step is smaller, spreading a
notch over more frames while finishing shortly after the last input. Input
classification, distance, reversal, cancellation and reduced-motion/native
bypass behavior retain their existing paths. No new handlers, animation chains,
timers, dependencies, native settings or persistence changes. The longer active
interval uses a few more frames; settled scrolling still has no scheduled work.

Production bundle `main-ChUb3Y_0.js`, SHA-256
`bc5a0b778d73fa6eb06e621d2ed3396601f0f104876c8ad498ff6f6fc7fe5b30`.
The CSS bundle remains `main-tPMih8Qp.css`.

The updated six deterministic tests check the gentler initial displacement and
finite finish as well as bursts, reversal, boundaries, cancellation and native
input classification. Sixteen real/synthetic wheel checks in private WebKit/Xvfb
pass, including immediate reversal/cancellation, exact distance, reduced motion,
nested/control bypass and zero idle callbacks. Refresh performance (10 checks)
and Overview geometry (13 checks) are rerun against the production bundle.
Visual inspection uses `workspace-ui/overview-wheel-refined.png` under reports.
Review covered the two changed parameters and retained lifecycle/input paths.

Current commands and evidence live under `.redunar-build/reports/`:

- `wheel-softer-unit.log`, `wheel-softer-input.log`,
  `wheel-softer-performance.log` and `wheel-softer-geometry.log`.
- `tools/validate.sh full`: `wheel-softer-full-console.log`, which prints the
  exact validation report path with adjacent source manifests. The wrapper
  rejects source changes during verification.
- `python3 output/tauri-redunar/build-native.py`:
  `wheel-softer-release-build.log`; `wheel-softer-runtime.sha256` identifies the
  rebuilt local executable. No package installation or regular app launch.

These fixtures verify behavior and refresh/idle work, not physical-device feel
or compositor FPS. Hardware touchpad/Wayland acceptance remains owner testing.
Earlier broad UI/native passes remain dated evidence of their own builds, not
new claims for this tuning. No installed app was restarted; no game, real capture,
system preferences or external services were used. Revert the duration/curve to
restore the preceding feel; there is no data migration.
