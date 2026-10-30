# NVIDIA beta readiness and diagnostic logging verification

Verified October 2, 2026 on CachyOS x86_64 with Rust 1.97.0, Node 26.10.0,
and WebKitGTK 2.52.6. Branch: `fix/nvidia-beta-readiness`, based on
`e589fc31abebb00396b152922c162a235db73ca9`. Earlier runtime-efficiency and
focus-transition changes remain in the working tree and were preserved.

The later [handoff and compatibility follow-up](2026-10-02-handoff-compatibility.md)
resolves the pending producer-sequence and compatibility-build work recorded
below. This report retains the evidence and limitations of its original source.

## Exact source and build

Final automated validation:
`.redunar-build/reports/20261002T192152Z-full.q30Ukj.log`.
Its initial and final source manifests match. The recorded `source_sha256` is
`bc5e49f8ad4473b2ef31b7f622fe50f68b964761ee04025de3295913fad10bfc`;
the `.source.json` file itself has SHA-256
`67685edd4042b79c67e535bf71a9db17786592a9be87a75d63a62d40a87a8ba6`.
This report and its TESTING link were added afterward; implementation files
were not changed after validation.

`python3 output/tauri-redunar/build-native.py` rebuilt the production frontend,
native app, capture libraries, Steam wrapper, shortcut helper, and update helper
from that source. The executable SHA-256 is
`41eba7a9a8688f4ebf6a67aaed52c4c8b37cdea8c8bdb59df5717a343214ef48`.
Capture-component hashes match `capture-components.json`; every staged native
payload matches its build artifact. The complete local artifact record is
`.redunar-build/reports/nvidia-readiness-native-artifacts.json`.

## Changed behavior and ownership

- The Vulkan encoder binds video-profile buffers and reconstruction references
  correctly, checks export/dedicated allocation requirements, uses external
  ownership barriers, and keeps the selected device UUID stable between probe
  and activation. Presentation resources have separate lifetime proof and a
  bounded retirement policy.
- Producer source rejection invalidates Replay readiness and queued unsubmitted
  frames. The capture receiver and replay worker coordinate rejection before
  handing out frames, retain failed release attempts, and allow recovery from a
  fresh supported source without clearing metrics or overlay state.
- Pipeline and runtime shutdown retain failures. Unfinished GPU input leases
  are not acknowledged after failed teardown; unsafe reactivation remains
  blocked. The worker stops its audio peer before joining on that failure path.
- Platform discovery distinguishes render GPUs from display-only devices.
  Hybrid and ambiguous NVIDIA recording paths remain gated; available AMD
  telemetry is preserved. NVML owns typed optional-sensor, device-loss,
  cleanup, and bounded recovery states, with stale readings cleared on loss.
- The shared service owns bounded asynchronous private diagnostic files. The
  native host records a running-executable fingerprint, and Settings reports
  actual logger health independently of the saved preference. Failed status
  reads clear stale active indications.
- Replay diagnostics include source/encoded/audio milestones, periodic
  aggregates, gaps, latency, recovery, save, and shutdown stages. Variable FPS
  reports actual intervals rather than fabricated fixed-cadence gaps. Numeric
  Vulkan and I/O failure facts survive conservative path redaction.
- The isolated acceptance runner explicitly enables the saved Beta and Debug
  preferences before constructing the production service. It covers MKV/MP4
  selection, bounded saved-video decode, and preservation of local diagnostic
  reports before private fixture cleanup.

Contracts live in [ARCHITECTURE](../../ARCHITECTURE.md),
[HARDWARE-SUPPORT](../../HARDWARE-SUPPORT.md), [REPLAY](../../REPLAY.md),
[PERFORMANCE](../../PERFORMANCE.md), and [DESIGN](../../DESIGN.md).
The [first hardware test](../../TESTING.md#nvidia-beta-first-hardware-test)
uses the official package's separate Beta access and Debug log switches.

## Current checks

| Check | Result on October 2, 2026 |
| --- | --- |
| `tools/validate.sh full` | Passed: 651 Rust tests, 12 ignored; 64 Node tests; 13 Python tooling tests; both Cargo checks and strict Clippy; formatting and production frontend build |
| Vulkan API/lifecycle fixtures | 92 passed, 2 hardware tests ignored; includes resource contracts, presentation retirement, source rejection, route bounds, and failed submission/cleanup |
| Shared service regressions | 255 passed, 6 hardware tests ignored; includes repeated failed teardown, withheld unfinished ACKs, rejection without a frame, saturated release queues, save-worker admission, and private logger failures |
| Workspace WebKit fixture | All 83 checks passed, including active logger followed by a rejected native status read |
| Visual inspection | Production Settings screenshots with unavailable logging inspected at normal and compact sizes; labels remain readable without overflow |
| Native production build | Passed, with matching sidecars and helper artifacts |
| Host package staging | `tools/stage-tauri-package.sh` passed using the explicit host release-root override; six native payload digests match the built artifacts |
| Metadata and source assets | Desktop metadata, offline AppStream validation, locked license inventory, and Replay shader check passed |
| Documentation | Affected local links, command references, and whitespace checked |

The hardware-dependent ignored tests were not enabled. Host staging output is
under `.redunar-build/packages/nvidia-readiness-host/staging`; it is not a
compatibility-qualified release candidate.

## Independent review and compatibility

Separate Vulkan, monitor, and logging reviewers checked production entry points,
failure handling, lifetime contracts, and privacy. Review corrections included
separating presentation completion from queue-idle proof, applying retirement
bounds to every route-creation path, preserving completed exports across later
submission failure, avoiding a game panic for retirement invariant failure,
prioritizing selected-source rejection, and stopping audio before failed worker
cleanup. Logging review corrected stale UI health, variable-cadence reporting,
and failure reason loss during redaction. The final integrated tree passed the
full validation after those changes.

No persisted profile/history/clip format migration, dependency addition, or
version change was introduced by this readiness work. Existing recordings and
the separate saved Beta/Debug preferences are preserved. Cleanup removes only
owned temporary fixture/media state. Logs remain local; sharing is explicit.

## Remaining evidence and limitations

- NVIDIA hardware, real-game capture, installed-app acceptance, and performance
  measurements were not run. Fake API/NVML fixtures do not certify NVIDIA
  recording, audio synchronization, or cross-driver OpenGL-to-Vulkan behavior.
- The Vulkan producer's normal retirement budget is two retired routes plus
  one overflow route while pausing exports. A separate defensive quarantine
  preserves ownership on an unexpected invariant failure. Missing independent
  presentation completion proof can retain generations until device destruction. Repeated swapchain
  replacements may make Replay unavailable while game presentation continues;
  unrestricted resize recovery is not established.
- Replay export tokens originate per process, while several session/import
  high-water marks still use numeric tokens across the session. A helper-to-game
  producer handoff with lower or overlapping sequences remains an existing
  limitation; this change does not establish collision-free multi-process
  token ownership. Handoff behavior needs separate follow-up and test evidence.
- Finite fence waits and cooperative shutdown cannot interrupt a native driver
  call that is stuck. Incomplete teardown withholds unfinished leases and blocks
  rearming rather than reporting successful cleanup.
- The Debian-12/glibc-2.36 compatibility image and its pinned base are not cached
  locally. The compatibility build and release/package gate were not run. The
  host build and staged payload must not be published as a qualified candidate.

On a teardown failure, retain the relevant local diagnostics, end the test,
and fully restart Redunar before trying another session. Do not bypass a GPU
gate or delete recordings to recover. Qualify the exact release candidate and
verify installed binary/sidecar hashes before the separately authorized human
hardware test. No app was installed or published during this work.
