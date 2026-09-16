# Tauri real-game acceptance checklist

Current manual checklist, reviewed September 13, 2026. Use a game/test mode
approved for capture and overlay testing. Scope and diagnostics are in
[REAL-GAME-TESTING.md](../../REAL-GAME-TESTING.md). This is not evidence of a
completed run; record the results separately with the exact build and host.

## Before launch

- [ ] Verify the intended binary/sidecars with
      `tools/check-installed-tauri-runtime.sh` when testing the installed app.
      Fully quit/reopen after an update; only one instance owns the coordinator.
- [ ] Record date, build, distribution, desktop/compositor, kernel, GPU/driver,
      display mode, selected game and its capture policy.
- [ ] With Close to tray disabled, verify no icon at startup. Enable/disable it
      repeatedly and verify immediate icon visibility changes. Check reopening
      when enabled and normal exit when disabled or no tray host is available.
- [ ] Save the desired recording rate, quality, and format before launch.
- [ ] Check optional bindings: fresh defaults are Shift+Tab (menu) and F8 (30s).
      With bindings assigned and keyboard access available, verify Active status.
      An empty set is valid and must not be reported as a setup failure.
- [ ] Verify exact game/Steam identity and inherited/custom profile state.
      A missing installation disables/rejects launch without deleting its entry.
- [ ] Record initial native diagnostics through isolated/internal tooling as
      appropriate; diagnostics are not a Settings card. Avoid private paths or
      tokens in shared output.

## Launch, visibility, and preferences

- [ ] Launch from Library and verify the owned game/session identity.
- [ ] Start with metrics hidden, then save Show in-game overlay on/off repeatedly.
      The prepared runtime stays available. Verify global inheritance and a
      per-game custom value without adding an Overview visibility button.
- [ ] Confirm recording settings remain locked until the owned game closes,
      while visibility-only saves work. Availability comes from runtime status,
      not the saved preference alone.
- [ ] Confirm frame/encode counters progress and a healthy populated replay
      buffer exists. Record drops, overruns, resize/reset behavior, and failures.
- [ ] Verify whether audio uses a game-owned node, an output fallback, or no
      source. Check controlled unrelated audio separately. Approved output fallback
      can include other apps and must not be labeled game-only; see [REPLAY](../../REPLAY.md).
- [ ] Open/close the replay menu by shortcut with the main window visible and
      hidden. Verify transparent exterior, no desktop sidebar/black bars, correct
      focus, Escape, repeated shortcut dismissal, and outside-click preference.
      Closing the menu must not quit Redunar.
- [ ] Open Preview replay menu in the app and check the separate dialog layout.
- [ ] Clear all shortcuts and save: keys no longer open/save, while buffering and
      app Save replay still work. Restart to verify clearing persists. Restore
      save-only and menu-only assignments and check each independently.
- [ ] Change a shortcut, then save outside-click preferences; the chord survives.
- [ ] Leave game A dirty, save game B or A's launch details, then return to A.
      Its draft survives. Save/Discard appears only when dirty at the viewport
      bottom and does not cover content or duplicate inside Library.

## Save, playback, and export

- [ ] Save from app/menu and an assigned shortcut. Native completion precedes
      inventory/success feedback. The bottom-left Moment saved pill also appears
      with metrics hidden; it reports the actual saved length.
- [ ] Select clips after scrolling the rail. Search, scroll, selected state and
      keyboard focus remain stable through loading, refresh, retry, and selection.
- [ ] Verify decoded video fills its matching aspect-ratio area without cropping,
      with controls below, real filename/game metadata, and no fabricated data.
- [ ] Seek/pause/resume repeatedly and confirm playhead follows decoded time.
      Failure gives a useful retry/error state rather than endless loading.
- [ ] Move both trim handles, verify In/Out/Duration, export, and confirm progress,
      cancellation, final file duration/audio, and preservation of the source.
- [ ] Verify clip deletion requires confirmation and affects only the selected
      owned clip. Inspect spacing and empty/final-item behavior.

## History and cleanup

- [ ] End session without killing the game; a second launch remains locked until
      the owned game exits. Verify natural exit/cleanup writes one history entry.
- [ ] Inspect FPS, frame time, temperatures and load against actual recorded
      observations. Labels/grid/traces/slider/inspector share coordinates and
      timestamps, both CPU/GPU traces use their assigned red shades, and missing
      values remain unavailable. The top tick is a bound, not an exact peak.
- [ ] Verify next launch unlocks, temporary resources are released, and prior
      clips/history/artwork remain.
- [ ] Repeat tray close/reopen and Quit; no stranded window, duplicate cleanup,
      unexpected privilege prompt, or leaked native worker remains.

Use [PERFORMANCE.md](../../PERFORMANCE.md) for matched overhead runs and
[RELEASE-CANDIDATE.md](RELEASE-CANDIDATE.md) for open release gates. Fixture/native
probe success is supporting evidence, not completion of this checklist.
