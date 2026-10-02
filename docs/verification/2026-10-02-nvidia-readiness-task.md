# NVIDIA beta readiness and diagnostic logging

- Problem: the opt-in production NVIDIA path needs valid Vulkan resource and
  synchronization contracts, deterministic hardware gates, and useful local
  diagnostics before its first owner-authorized hardware recording test.
- Branch: `fix/nvidia-beta-readiness`, based on `e589fc3` plus the preserved
  runtime-efficiency and focus-transition working-tree changes. Independent
  Vulkan and monitor workers use separate worktrees with that same baseline.
- Acceptance: correct video profile/reference binding, safe presentation and
  external-buffer reuse, stable device selection, conservative capability
  rejection, tested NVML failure/cleanup, render-device topology gates, and
  explicit Beta opt-in in the acceptance fixtures.
- Diagnostics: bounded asynchronous private files, millisecond/monotonic
  timing, actionable allowlisted capability codes, startup and recording
  summaries, explicit logger health, rotation/failure/shutdown tests. Preserve
  the separate saved Beta and Debug log switches and restart semantics.
- Owners: Vulkan producer/encoder, platform/NVML monitor, shared service
  logger and replay lifecycle, Tauri settings/lifetime, isolated test runners.
- Existing profile formats and user recordings remain unchanged. GPU failures
  must not silently select another device or enable software recording.
- Checks: focused offline fake-hardware and API-contract tests, full documented
  validation, production UI build and isolated WebKit visual checks, relevant
  packaging checks, separate integrated review. Record current results with
  the exact source manifest in the final verification report.
- Deferred evidence: no installation, real-game launch, NVIDIA hardware run,
  release publication, or verified cross-vendor support is implied. Driver
  hangs cannot be made interruptible merely by timing out a Rust thread; any
  remaining lifecycle limitation must be explicit.
