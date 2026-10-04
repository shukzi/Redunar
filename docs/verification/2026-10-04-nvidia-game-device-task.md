# NVIDIA beta device attribution

- Problem: a tester's RTX 3090 Ti and Intel i915 render device are correctly
  discovered, but the app-wide single-render-device gate withholds NVIDIA
  telemetry and Replay before driver capabilities can be tested.
- Branch: `fix/nvidia-game-device`, based on
  `f6cd5676de77e367f43f769fe03b6cd88782a1ff`; independent checkout under
  `worktrees/nvidia-game-device` with private agent instructions.
- Acceptance: Beta access discovers one unambiguous NVIDIA render GPU alongside
  Intel; existing Vulkan and desktop OpenGL capture report their actual device
  identity; Replay matches that identity before import/encode; game telemetry
  and history select the corresponding hardware rather than the first card.
- Ownership: platform discovery/NVML, the bounded capture protocol and both
  game producers, daemon capture/encoder, and native session telemetry.
- Keep missing identity, duplicate/unknown device attribution, unsupported
  Vulkan Video/import capabilities, source replacement, and teardown failures
  honest and fail closed. Preserve CPU/AMD monitoring, profiles, original
  recordings, per-process release ownership, and nonblocking presentation.
- Scope: existing APIs, local beta diagnostics, fake-hardware regressions, and
  production/package checks. Multiple indistinguishable same-vendor cards stay
  unavailable for telemetry/GBM attribution. No install, live game, firmware
  change, external contact, or publication is authorized by this implementation.
- Checks: focused protocol/discovery/producer/service regressions, full checkout
  validation, native sidecar/build consistency and applicable package checks,
  plus a separate lifecycle/privacy review pass. NVIDIA hardware qualification
  requires the tester's separately authorized run of a matching candidate.
- Result and limitations: [October 4 verification](2026-10-04-nvidia-game-device.md).
