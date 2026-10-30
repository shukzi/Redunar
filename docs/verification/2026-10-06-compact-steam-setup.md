# Compact Steam setup verification

October 6, 2026. Branch `fix/compact-steam-setup`, base `4c5723c` plus the
[approved Compact task](2026-10-06-compact-steam-setup-task.md). Full validation
and the Debian-compatible build used the identical source manifest SHA-256
`26bb7e5fb973e2f2269a6faae90063595b342d36ec9d46f56b7f2111ecc731f4`.
This evidence report was added afterward; production sources are unchanged.

The per-game panel now shows Play from Steam, one paste instruction, the exact
native launch option with Copy, and a short automatic startup note. Routine
unconfigured and running-Steam states have no warning or retry control.
Only verified native setup evidence shows Launch option configured and the Play
instruction. Other ambiguous reasons, unavailable setup and failed checks keep
their explanation and Check again. No native detector, capture gate, Steam
configuration, profile persistence or session lifecycle changed.

Checks passed in `worktrees/steam-setup-discovery`:

- `tools/validate.sh full`, exit 0:
  `.redunar-build/reports/20261006T143745Z-full.vXDOl2.log`. Both Cargo
  workspaces, Rust/Node tests, strict Clippy, formatting and production frontend
  build passed. Nine focused setup tests cover absent/dynamic retry controls,
  escaped errors/options, recovery, detached panels, stale success/failure
  responses and unchanged field preservation.
- Production WebKitGTK workspace fixture, 96 checks passed:
  `.redunar-build/reports/compact-steam-visual.log`. Inspected the setup,
  configured and 640-pixel compact screenshots under `target/ui-review`.
  Exact Copy, both Library tabs, direct-game omission, selection/focus,
  unrelated profile drafts and error recovery passed. The initial new selection
  test attempted to focus a field before a profile-change render had completed;
  waiting for the native setup response corrected the fixture timing. The final
  full validation includes that correction. Xvfb's DRI3 warning does not establish
  hardware acceleration; this run uses private command fixtures.
- `tools/build-linux-release.sh`, exit 0:
  `.redunar-build/reports/compact-steam-compatible-build.log`. Debian 12 offline
  build passed the glibc 2.36 ceiling; all six runtime artifacts require at most
  glibc 2.34. `runtime.sha256` verification passed. App SHA-256:
  `d2a1fd8cdaf92e0687756460c9ef3c24f168f3d2346f6fa2f1c24833d4fe313f`.
  Wrapper, helpers and capture libraries are byte-identical to the prior local
  build. Manifests remain in `.redunar-build/linux`.

The prior app was closed before launch. The local candidate started without a
shortcut-helper override. Fresh app PID 740144 and helper PID 740193 loaded
executables matching the candidate hashes; the local startup output was empty.
This is a local build launch, not an RPM installation. The owner controls the
ARC Raiders Steam launch and Replay check; no actual game was launched here.
NVIDIA hardware acceptance remains pending.

A final review traced Library render/focus/actions, native status serialization,
service-generated options, the last-loaded stylesheet and async request guards.
Only the setup content changes during status refresh; profile drafts and native
ownership remain intact. No unresolved review issue was found. Recovery is a
revert of this UI-only change. No installation, package publication or GitHub
write was performed.
