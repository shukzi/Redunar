# Steam setup discovery verification

October 6, 2026. Branch `fix/steam-setup-discovery`, base `84231a6` plus the
[task changes](2026-10-06-steam-setup-task.md). Full validation and the Debian 12
production build used source SHA-256
`7dc4b96018a6b9832531058657a5055ef43677c94a8fa7b1ca6e0af10538a446`.
The subsequent evidence/link updates and Markdown wrapping are documentation only.

## Result and ownership

Idle Overview offers **Set up Steam capture**. Each selected Steam game shows
**Play from Steam** above both Library tabs, with native setup status, one-time
instructions, its exact read-only Launch Options value and a Copy button.
Configured games retain the value; unconfirmed checks expose the native reason,
and unavailable/error checks retain retry without an unusable copy action.
Direct games omit the panel. Import selects the game using the existing flow,
so the same panel appears without launching a game or opening Launch matching.

`ui/steam-setup.mjs` owns read-only rendering and current-panel request
ownership. Detached panels and superseded checks ignore late results. Unchanged
checks preserve the existing option field and manual selection. The UI invokes
the existing `steam_setup_status`; catalog/service/platform own identity,
validation and option generation. No new persistence or Steam writes exist.
The existing copy fallback now removes its temporary textarea after failure.

## Checks

All reports are checkout-relative under `.redunar-build/reports/`:

- `20261006T134452Z-full.mD4P4L.log`: `tools/validate.sh full`, exit 0. Both
  Cargo workspaces, Rust and Node suites, formatting/syntax, production UI build
  and strict Clippy. Five focused setup tests cover detached panels, overlapping
  responses, escaped errors/retry, unavailable options and unchanged-field reuse.
- `steam-visual-qualified.log`: production WebKitGTK workspace fixture, 94
  checks passed with isolated backend/state. Setup status/copy, both Library tabs,
  direct-game omission, unavailable/error retries, drafts and keyboard selection
  passed. Normal 1440-pixel and narrow 640-pixel screenshots were inspected:
  `target/ui-review/steam-play-{setup,configured,compact,overview}.png`.
- `steam-compatible-build.log`: offline Debian 12 production build, exit 0.
  The six runtime components require at most glibc 2.34, within the 2.36 ceiling.
  The runtime SHA-256 manifest check passed.

The first WebKit run passed the changed setup checks but reached the pre-existing
RAM clearing assertion before a scheduled poll. The fixture now waits for that
actual cleared state with a five-second deadline instead of a fixed 1.4-second
sleep; its measurement assertions are unchanged. The final complete run passed.

## Runtime reference and review

App SHA-256:
`ae3bd491efd13587517b204b992c2c52810a9c3dfc1b968068ce8f5723e19c39`.
The Steam wrapper, both capture libraries and both helpers have identical hashes
to the [October 5 combined candidate](2026-10-05-combined-live-check.md).
No capture, encoder, NVIDIA attribution, background process or saved format code
changed. A separate review pass traced initial render, tab/selection changes,
import, focus refresh, errors, retries, clipboard cleanup and native ownership.
No unresolved implementation issue was found.

After the owner fully quit the prior app, a fresh local process loaded this
app hash and the matching shortcut-helper hash. No helper override was needed.
The checked local build is retained in `worktrees/steam-setup-discovery` for the
owner's ARC Raiders test. No package was installed or published and no game was
launched by this verification. Actual Steam/game and NVIDIA acceptance remain
separate from these fixture and build results.
