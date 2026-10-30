# Replay menu dismissal — October 9, 2026

- Problem: the owner reports the in-game menu disappearing immediately after
  opening. Mouse handles stay open while closed, but their queues are only read
  while the menu is open; old gameplay clicks can reach menu hit-testing.
- Live read-only follow-up: the running helper retains three nonblocking mouse
  handles, two pointing to removed input nodes. Failed-grab view-only fallback
  still reads those handles in the old implementation and can immediately close
  the menu on a read error. Live telemetry remains Buffering/save-ready; the
  menu shortcut is Alt+Shift+Z. No live command or input event was injected.
- Acceptance: discard queued pointer input after acquiring every mouse; only new
  menu input may reach the daemon. View-only fallback must send heartbeats without
  forwarding input from unowned mice. Preserve Escape, repeat-toggle, save,
  watchdog, render-target loss and cleanup behavior.
  Refresh mouse discovery before each opening so removed handles are replaced;
  never replace devices while the menu owns them.
- Branch: `fix/replay-menu-dismissal`, base `9a3be61`; existing clean source
  checkout, checkout-owned `.redunar-build` outputs.
- Ownership: native shortcut configuration → `redunar-hotkey-helper` → private
  replay control socket → daemon menu state/telemetry → Vulkan/OpenGL renderer.
  Pointer acquisition and queue cleanup belong to the helper.
- No preference, inheritance, persistence, ABI, renderer styling or dependency
  changes. Failed acquisition/drain releases all mice and retains view-only mode.
  Queue draining must be bounded and use the existing nonblocking handles.
- Checks: focused fake mouse/socket regressions, the owner's Alt+Shift+Z chord
  through the real keyboard monitor with composite duplicate edges and releases,
  full validation, offline release
  helper build and package staging/build fixtures. No install, real-game run,
  device-permission change or external service contact.
- Limitation: the reported game and installed build are not yet identified;
  this addresses a reproduced code path, not confirmed real-game acceptance.
- Recovery: revert helper changes; recordings/preferences require no migration.
