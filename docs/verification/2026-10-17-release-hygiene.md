# Remaining release hygiene reviewed — October 17, 2026

Branch `chore/review-release-hygiene`, base `160fa5b`. Scope and recovery are in
the [task brief](2026-10-17-release-hygiene-task.md). This integrates the local
release/upload cleanup left separate during the workspace integration.

## Findings and final changes

- Tauri JSON still declared 0.1.23 although the package specification, native
  Cargo manifest and latest AppStream release already declared 0.1.24. Correct
  the config and check it in `tools/check-release-inputs.sh`. Six temporary
  Python fixtures cover matching versions, stale/missing/malformed JSON, other
  component mismatches and invalid tags. The stale-config test fails against
  the former gate, then passes against the new gate.
- Release/package guidance now compares installed bytes with the exact native
  package and distinguishes build, installation and fresh-process acceptance.
  The existing installed-runtime script is explicitly identified as RPM-only.
  Historical September evidence keeps its recorded hashes/results without
  claiming those packages describe the currently installed runtime or exposing
  repository-relative links to external local build bundles.
- Packaging guidance matches the fixed polkit update helper and policy already
  in the stager and package specification. These edits add no privileges or
  runtime behavior. Review additionally corrects the post-install description
  to include both keyboard and mouse udev triggers.
- Tray documentation matches `tray_native/` GIO/GLib StatusNotifierItem/DBusMenu
  ownership and the actual Linux dependency graph. All legal notices and the
  generated dependency inventory remain intact; optional lockfile entries do
  not imply an active AppIndicator dependency.
- The listed deletion of `tools/xcb-resize-window.c` is accepted after tracing
  production, explicit build inputs, fixture scripts, window diagnostics,
  staging and persistence/licensing obligations. Its only executable reference
  was its own definition. The replay runner compiles the retained finite Vulkan
  scene explicitly; the geometry diagnostic uses its own Python Xlib bindings.
  Neither consumes this helper. Retain those active tools and every notice.
  Recover the exact helper from `160fa5b:tools/xcb-resize-window.c` if needed.

No blocking findings remain. No UI interaction, scrolling, capture lifecycle,
profile inheritance, persisted format or installed component changes here.

## Verification and limitations

Focused results use `.redunar-build/reports/remaining-review-*`:

- `check-release-inputs.sh v0.1.24` succeeds; all six new fixture tests pass.
  A private old-gate comparison demonstrates the stale-config regression.
- `tools/test-installer.sh` passes using local file assets and fake package
  managers, including Arch/CachyOS audio-provider paths and rejected signatures/
  checksums. It neither contacts release servers nor installs anything.
- `CARGO_NET_OFFLINE=true python3 tools/generate-license-inventory.py --check`
  passes. The locked offline Linux normal dependency tree contains no
  tray-icon, libappindicator or libloading; retained notices are unchanged.
- All local link targets in the affected existing documentation resolve.
- Private staging of the existing local native build passes exact byte checks
  for executable/sidecars/helpers, update policy, udev rule and dependency
  notices. Modes, absence of setuid/setgid bits/capabilities, and absence of
  the experimental KMS helper are verified. The private stage is cleaned up.
  Payload hashes are in `remaining-review-staging.json`. This is not installed
  runtime acceptance or a newly qualified distributable package.
- The full documented gate runs on the final candidate commit before local
  fast-forward integration. `remaining-review-full-console.log` identifies its
  dated report and before/final source manifests; `remaining-review-integration.json`
  records the exact integrated commit, outcome and unchanged source audit.

No remotes are changed or contacted. Nothing is pushed, published, installed or
restarted; no live game, actual package manager, renderer or hardware setting is
used. Full release-candidate qualification, package-format builds and actual
installed/runtime or cross-distribution acceptance remain separate work.
