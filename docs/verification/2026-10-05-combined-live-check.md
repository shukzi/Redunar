# Combined build live verification — October 5, 2026

Task: [combined live check](2026-10-05-combined-live-check-task.md).
Branch `fix/live-replay-device-identity`, base `5b919c8`, package version `0.1.13`.
Includes the NVIDIA game-device attribution and Steam Play background preparation.
The compatibility/package build used source SHA-256
`37c2fc85a3af411204218a843a40f1bd0041c2e71f7005ffd95734293046f858`.
Later edits affect acceptance-test retry handling and documentation only.

## Findings and fixes

The distro's vkcubepp explicitly requests Vulkan 1.0, confirmed by the local
loader's `vkCreateInstance` log. The Vulkan 1.1 Replay gate predates this task;
the former acceptance fixture no longer exercised a supported Replay source.
It also panicked before explicitly ending its Replay coordinator. Replace it
with an independently implemented, bounded Vulkan 1.1 scene, a failure guard,
normal scene teardown and a process deadline after compilation.

The new scene exposed a real crash: with a globally linked Vulkan loader, ELF
symbol preemption replaced the layer's negotiated proc-address hooks with loader
functions. GPU enumeration recursed through the loader/Mesa device-selection
layer. The original packaged library fails the competing-symbol fixture at
`vkGetInstanceProcAddr`. A Linux cdylib-only `-Bsymbolic-functions` link option
keeps its own hook addresses local; next-layer calls retain captured pointers.
The fixed library passes the fixture and the same live scene.

Background startup clears the local shortcut-helper environment override. The
native app now finds its matching adjacent or unpacked `libexec` helper. The
helper verifies the same user and its own inode in that fixed package layout.
No input-device permission is granted by this lookup.

RPM inspection now uses a private temporary database. Capturing the complete
payload list before filtering removes a pipefail/SIGPIPE false failure without
weakening the input-rule, scriptlet, permission or privileged-path checks.

## Evidence

Host: x86_64 CachyOS, RX 6800 XT/Navi 21, RADV Mesa 26.2.4, Vulkan device 1.4.354,
XCB windows in the owner's Wayland desktop. Ordinary checks use private fake
hardware/state; the owner authorized the separate finite local hardware checks.
No real game was launched by the agent.

- `tools/validate.sh full`: both workspaces, production frontend, Rust/Node
  suites, formatting, syntax and strict Clippy passed after the runtime fixes.
  The final repeat after fixture/document completion is retained in
  `.redunar-build/reports/combined-final-full-console.log`.
- `tools/check-tauri-release.sh`: passed compatibility compilation, licenses,
  metadata, the competing-symbol regression, payloads, DEB/portable/Arch/
  openSUSE/Fedora package checks and local test signatures. All six runtime
  components require glibc at most 2.34. No installation occurred.
- Production WebKitGTK fixture: 84 checks passed; Steam setup screenshot
  visually reviewed. `.redunar-build/reports/combined-visual.log`.
- Tauri Vulkan Replay runner: MKV and MP4 passed, including the real native
  supervisor, available AMD identity, fifteen seconds of buffered history,
  committed/indexed output, FFmpeg video decode, one history record and cleanup.
  Both runs retried the spool's explicit busy response once before acceptance.
  The test permits ten bounded retries; other errors still fail immediately.
  Reports: `combined-vulkan-replay-console.log` and
  `combined-vulkan-replay-mp4-console.log` under `.redunar-build/reports/`.
- OpenGL production probes: GLX with metrics/MKV and SDL desktop OpenGL with
  hidden metrics/MP4 passed buffering, saving, menu control, visibility changes,
  saved notice, natural completion and worker/FD cleanup. Actual H.264 and Opus
  streams decoded with FFmpeg; private media was removed afterward. Reports:
  `combined-opengl-{glx,sdl}-console.log` and matching decode logs in that folder.

Final packaged runtime SHA-256:

| Component | SHA-256 |
| --- | --- |
| App | `eae1518dfb57fc9fa704ad9daf1460f4f39a7d4d14bf42a40000413e89b17cfd` |
| Steam wrapper | `0c92d170bacb3fa207d3e24e8e5d1311ecb11f368500de40ca0daf046c9dc609` |
| Shortcut helper | `895ae3e1f4ded46aa784c69a53226d26e33e5c4a01c6898f9bc81483cf9aa256` |
| Update helper | `72c323c520c2fbc831487b7ffd1c32dea4b256757e690c3a52da7568303a2452` |
| Vulkan layer | `b01baa6ecee4ff579ec2429c106d0c0b14c92f2632d436248162a0c4d9eaf90d` |
| OpenGL interposer | `fc846ceec6b1c16ad05b05368c82b746b2da0fae91dfd882b58e9a03e3388695` |

## Separate lifecycle, compatibility and privacy review

Reviewed the loader negotiation and instance/device proc dispatch, literal launch
arguments, helper UID/parent/inode checks, inherited environment removal, scoped
state/clip cleanup, cancellation deadlines and package-query isolation. The
symbol fixture accesses no device and the Vulkan scene never changes a game's
API. Production data formats, capability gates, encoder matching and saved
profiles remain unchanged. No new dependency or copied third-party code.

The first unsupported-scene run and the symbol crash were retained as failures;
they do not become historical passes. The corrected failure guard completed a
no-frame test with an assertion failure and joined cleanup. Successful Vulkan
teardown can emit a source rejection/recovery before capture completion; the
retained log still reports that transition and `shutdown_complete` rather than
hiding it. Spool contention can still return a retry response in production.

NVIDIA hardware, hybrid topology, actual Steam Play background startup, PEAK,
other distributions' installed runtime and visual/audio game acceptance remain
unverified. Vulkan's isolated runtime had no desktop audio connection; the
OpenGL checks separately decoded audio, without claiming an audible game test.
The owner will test PEAK. Reopen the matching local app after checks and verify
its running executable hash; this is a local candidate, not an installed update.
No GitHub text, source, release or telemetry was uploaded.
