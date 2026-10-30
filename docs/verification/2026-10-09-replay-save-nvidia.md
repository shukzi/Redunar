# Replay save layout and NVIDIA extension discovery — October 9, 2026

Source base: `0ede9f82aaf56eb2eed2233cc9cfb5e463cdd70b` (0.1.19), local branch
`fix/replay-save-layout`. The fix qualification below preceded the owner’s
v0.1.20 release request. No installation or game run was part of these checks.

## Findings and changes

The Save recent gameplay row treated the selected duration's shortcut badge as
an independent space-between child. Showing that badge moved the controls toward
the center. The badge now belongs to the controls, with space reserved for the
longest assigned save-duration shortcut. All-cleared bindings omit that space.

An inactive recorder with an active game and no explicit rejection now exposes
pending-frame copy separately from hardware/source unavailability. Failure and
explicit rejection retain error priority; pending recording cannot save. Encoder
startup failure says the encoder failed; packaging failure identifies packaging.

The supplied tester log identifies 0.1.19 and confirms an NVIDIA Vulkan producer
and first source frame, followed by repeated device-extension enumeration result
`5` (`VK_INCOMPLETE`) before encoder creation, with no encoded packets. The older
0.1.14 dimension rejection is separate historical evidence. Current encoder
queries truncated the requested allocation to 256 entries; producer queries
allowed 1,024. Both now use one complete query with the existing 1,024-entry
producer bound and four attempts for count/fill growth. Partial, oversized or
persistently unstable lists fail closed. Required extensions, source/driver UUID
matching and all remaining encoder/import gates are retained.

## Verification

- Focused fake-driver tests: 5 passed, no GPU access. Cover lists above 256,
  count/fill growth and shrinkage, zero entries, bounded retries and allocation,
  invalid written counts and genuine error results.
- Focused Node Replay status checks: 5 passed.
- Production frontend build and WebKit workspace fixture: 102 checks passed.
  Visually inspected assigned/unassigned-duration and compact save rows.
- `tools/validate.sh release`: passed both locked/offline Rust workspaces,
  production frontend/Node checks, strict Clippy, license checks, compatibility
  build, Vulkan symbol-binding fixture and package/installer checks. Tauri app
  tests: 114 passed, 2 hardware tests ignored.
- Documentation links: 66 local references resolved; final diff check passed.

Validation log: `.redunar-build/reports/20261009T184424Z-release.md6NhT.log` (result 0).
Validated source manifest: `565fdda652c1fc142dd2900540dfdc5bade0eb1f53fdbe026ce9b32d96d0e525`.
Compatibility build manifest: `565fdda652c1fc142dd2900540dfdc5bade0eb1f53fdbe026ce9b32d96d0e525`.
UI log: `.redunar-build/reports/20261009-replay-save-webview-final.log`.
Screenshots: `target/ui-review/replay-save-{assigned,unassigned,compact,cleared}.png`
and `replay-{waiting,encoder-failure}.png`. All six were visually inspected.
Final evidence updates after qualification change documentation only; code and
fixtures remain identical to the validated manifest.

A separate review pass traced producer extension enablement, encoder dispatch and
required-extension checks, native runtime status, inherited shortcut maps and
effective stylesheet order. No persistence/wire format, capability gate or
cleanup ownership changed. Bounded failures preserve diagnostic results; existing
lifecycle/cancellation/ownership tests passed. Reverting this local branch returns
to the base behavior without a data migration.

Generated logs, binaries and screenshots stay in ignored build/review directories.
Package checks built a temporary RPM; no package was installed or published.

## Limits and follow-up

These checks exercise fake drivers and private fixture state. They do not prove
NVIDIA import, encoding, save/playback or sustained pacing. A matching tester
build must be separately installed and tested, confirming progress beyond device
extension discovery and playable encoded output. Publication is handled separately after v0.1.20 candidate qualification and
owner review of the exact GitHub wording.
The separate CachyOS/X11 game-start investigation is parked and excluded.
