# Steam startup and automatic exit task — October 9, 2026

- Base: `ea6fa3bf85ffa8a52b8684ca02c4ad5a0e25769c` (v0.1.18), branch
  `fix/steam-background-auto-exit`; clean checkout before work.
- Initial acceptance (refined below after local testing): Steam startup respects Close to tray. Enabled uses a hidden
  window/tray; disabled uses a minimized taskbar window without requesting focus.
  After natural game exit and successful session/history cleanup, automatically
  started Redunar exits if Close to tray is disabled and the user has not opened
  its window. Existing/manual owners and user-opened windows remain running.
- Ownership: native background startup, tray/window activation and session
  supervisor. Native completion notifications must work without webview polling.
- Failures: pending cleanup/history reveals the window; stale completion cannot
  quit a subsequent game; End must retain ownership while a game is live.
- Persistence: no preference/schema/migration change. Ordinary fixtures use
  isolated state, fake hardware and test-owned processes.
- Checks: targeted Tauri regression tests, full documented validation, production
  build and isolated native window inspection. No installation, real-game test,
  version bump or GitHub publication is authorized by this implementation task.
- Recovery: revert this focused change; persisted settings and recordings remain
  compatible. Desktop focus/minimize behavior needs separate compositor acceptance.

Results are recorded in the [verification report](2026-10-09-steam-auto-exit.md).
Current follow-up results are in the [temporary-tray report](2026-10-09-steam-temporary-tray.md).

## Local-test follow-up

The owner confirmed tray-enabled startup and retention after game exit, but
reported that tray-disabled startup did not quit after game exit on niri/DMS.
Focus-based retention must be replaced with deliberate native window input or
explicit Open actions. Automatic focus at either startup or game exit must not
retain an unused automatic owner. Preserve cleanup/history gating and verify
both focus-without-input and actual webview input in isolated native fixtures.
The owner's local-test authorization remains in effect; do not publish this work.

The owner then chose hidden tray startup as the visually cleaner behavior even
with Close to tray disabled. Automatic startup must use a temporary tray icon
without changing that preference; natural exit removes it and quits the unused
owner. Open reveals the window, removes a disabled preference's temporary icon
and retains the owner. Manual startup retains the usual saved tray behavior.
Missing-host fallback must remain reachable and ignore compositor focus alone.

Closing a foreground window during a live owned game must also return to a
temporary tray entry without ending Replay/session ownership. Off closes the
background owner after successful game completion; on keeps it in the tray.
This applies to manual owners once their window is closed. Open cancels the
pending close/exit; closing again re-arms it. Without a usable tray, keep the
window reachable. Idle close and explicit Quit retain their ordinary meanings.
