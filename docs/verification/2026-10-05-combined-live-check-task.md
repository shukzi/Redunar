# Combined build live-check task

- Problem: the combined NVIDIA attribution and Steam Play build failed its local
  Replay acceptance probe. The distro's vkcubepp cannot request a specific API;
  the producer rejects its export, and the failed fixture does not finish cleanly.
  Steam background startup also clears the local shortcut-helper override.
  The Vulkan 1.1 scene then reproduced a real ELF symbol-preemption crash with
  the globally linked Vulkan loader/Mesa layer. Packaging exposed unisolated RPM
  queries and a pipefail/SIGPIPE check; include these acceptance failures.
- Branch/worktree: `fix/live-replay-device-identity`, based on `5b919c8`.
- Acceptance: a supported, bounded generated Vulkan scene must save a fifteen
  second clip through the production native supervisor, decode it, and release
  its state. Failure paths must clean up before assertions. Local and unpacked
  package startup must find their matching shortcut helper without game env vars.
- Ownership: native shortcut lookup; isolated native acceptance fixture/runner;
  a standalone generated Vulkan scene. Preserve production capability gates and
  the game's requested API, saved profiles, existing recordings and shortcuts.
  Bind the Vulkan cdylib's internal hook addresses locally, retain next-layer
  dispatch, and add a competing-symbol fixture without GPU access. Keep package
  queries on a private RPM database and capture file lists before filtering.
- Checks: focused failure/lifecycle and helper-path fixtures, both workspace
  tests and strict Clippy, production frontend, package/compatibility checks,
  bounded live Vulkan and OpenGL probes on this host's AMD GPU.
- Manual scope: the owner authorized local build checks and reopening the app.
  The owner will test PEAK personally. No real game, installation, NVIDIA
  hardware, external service, or GitHub publication is part of this work.
- Recovery: retain previous candidate and diagnostic reports; kill only owned
  generated scenes on failure. Reopen the qualified combined candidate after
  checks, preserving normal user state.
