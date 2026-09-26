# Development workflow verification — September 26, 2026

## Build reference

Branch: `chore/development-workflow`, based on
`abc191865886a66b2f6fda9f93dd62d855f5e466`.

The final implementation passed `tools/validate.sh release` with working-source
manifest SHA-256:

```text
0fde21f3ffd19881e3f8c148f3aaa4d082c96fea1eee726c0ee215632a52afd4
```

Local report: `.redunar-build/reports/20260926T202245Z-release.zS0zah.log`.
The adjacent `.source.json` and `.final-source.json` agree. This summary was
added afterward as a documentation-only change. Later documentation and workflow
label cleanup did not change executable code; the results below describe the
implementation snapshot identified above.

## Results

- 12 workflow fixture tests passed: modified/new/deleted source snapshots,
  ignored/private files, symlink refusal, independent copies, build locks,
  packaging locks, failed rebuilds, payload tampering, dirty/invalid candidates,
  and cancellation cleanup.
- Shared Rust workspace: 440 tests passed, 10 hardware-dependent tests ignored.
- Tauri workspace: 87 application tests plus 9 and 20 example-module tests
  passed; two hardware-dependent tests ignored.
- 57 frontend tests passed. Both Rust workspaces passed checks, formatting, and
  strict Clippy without the earlier pointer-comparison allowance.
- Frontend production build passed; the generated version matches the package
  specification, and the source HTML retains its version placeholder.
- Debian 12 compatibility build passed. All six runtime binaries/libraries
  require at most glibc 2.34, below the 2.36 ceiling. The build records the exact
  container image ID and source manifest.
- Full package gate passed: Fedora RPM, Debian DEB, Arch package, openSUSE RPM,
  portable archive, installer fixtures, licenses, metadata, temporary signing
  and signature checks, staged contents, and privilege boundaries.
- Workflow YAML and embedded shell blocks parsed locally; candidate dependencies,
  read-only default permissions, secret separation, and manual publication
  defaults were inspected. Local Markdown links and diff whitespace passed.

## Findings resolved during verification

The UI tests asserted the old release VERSION-writing command and source-HTML
mutation. They now check the candidate writer and Vite version transformation.
Two existing OpenGL layout tests expected unencoded palette values even though
the renderer already encodes them before writing RGBA8. Their expected values
were corrected without changing rendering. Two pointer comparisons were changed
to `std::ptr::eq` so strict Clippy passes without a blanket allowance.

The initial nested temporary directory exceeded a Unix socket path limit. The
wrapper now uses the short system temporary root; tests own their private child
directories. The successful full gate used an environment with Unix socket support and
access to the existing Podman runtime.

## Scope and remaining activation

No app package was installed, no real game or live graphics/audio probe was run,
and no hosted workflow or publication step was run. No new product
version or release tag was created. Package checks do not establish runtime
support on other distributions or hardware.

GitHub runner execution and remote protections remain unverified until the
workflow is deployed, repository settings are configured, and hosted validation
completes.
The clean-tag candidate wrapper's rejection paths are fixture-tested; its
underlying validation and all package builders passed locally. A complete tagged
candidate dispatch and production signing/publication have not been exercised.
