# Tauri real-game acceptance checklist

Current manual checklist, reviewed September 24, 2026. Use a game/test mode
approved for capture and overlay testing. Scope and diagnostics are in
[REAL-GAME-TESTING.md](../../REAL-GAME-TESTING.md). This is not evidence of a
completed run; record the results separately with the exact build and host.

## Stardew Valley real-game follow-up

On September 23, Stardew Valley launched through Redunar's native Steam wrapper
had no game sound; a direct Steam launch remained audible. The Redunar launch
loaded SDL2 and libpulse; its `PULSE_SERVER` named the pressure-vessel Pulse
socket, which existed and accepted a connection, but no PipeWire sink-input
appeared. Game preferences were unmuted and
`SDL_DYNAMIC_API` was unset after its invalid Redunar override was removed. The
retest still had no sound, so the environment cleanup is not a proven fix.
The absence of an FAudio mapping was observed but is not established as the
cause. Do not mark this issue resolved from synthetic audio or overlay tests.
Any further real-game launch requires owner authorization for that launch.

On September 24, the owner first installed a local diagnostic RPM containing
an SDL pass-through fallback and reported that live sound was still missing.
Instant Replay was not tested with that package.

On September 24, a source audit found a concrete loader-state defect in the
OpenGL interposer: after any successful `dlopen`, its internal SDL symbol
discovery could leave a missing-symbol error pending. Stardew's bundled OpenAL
checks `dlerror()` immediately after loading `libpulse.so.0` and rejects the
handle when that error is non-null. The interposer now clears errors from its
internal probes after successful loads. This matches the observed mapped-
`libpulse`/no-sink-input behavior. The owner subsequently reported that live
Stardew sound returned with the loader-state-fix RPM. Read-only verification
confirmed the installed OpenGL sidecar matches that RPM payload (SHA-256
`625f23872ceb9009c904b3503070ebb80745091330372ee607328ee987653099`). Instant
Replay audio has not yet been tested, so Replay audio acceptance remains open.

The owner confirms Show in-game overlay was enabled, but metrics/overlay were
not visible. No completed Replay save was reported; the saved pill appears only
after a Replay save completes. No game process remained for a live status
check. The supplied Replay-page screenshot shows `Replay unavailable`, `0 s`
buffered, `0 received`, and `0 encoded`. Its “try another display mode” copy is
the generic no-frames message; it does not establish a display-mode rejection.
The Steam console log records `activation claimed` at 13:21:28, about ten
minutes before the screenshot timestamp. If this was the same launch, Steam
accepted Redunar's activation and received its environment updates; the
remaining likely boundary is producer startup/presentation or socket access.
The log cannot tie that line to a game PID, so this is not conclusive. On a
live run, correlate the wrapper result with the game PID, whether the
interposer is mapped, and the capture phase/counters before changing display
settings or code.

Later on September 24, the owner supplied screenshots from a Redunar-launched
Stardew session showing `Metrics enabled · replay enabled`, live system
readings, failed frame pacing, and the Replay strip at `0 received · 0 encoded`.
Overview reported: “Steam setup connected, but the frame-metrics producer
could not reach Redunar within 300 seconds.” During that run the session's
telemetry file was valid and marked metrics visible, and Steam logged activation
claimed at 13:59:34. The matching capture directory was removed shortly after
the five-minute timeout, so no live process mapping remains to inspect. Steam
also logged a wrong-ELF-class warning for the 64-bit interposer during launch;
the game's executable is 64-bit, so that warning alone does not prove the game
rejected the library. Source inspection shows the OpenGL producer sends its
hello only after an accepted presentation hook; the exact failure remains
unresolved among library loading, hook interception, presentation selection,
and socket access. Do not treat overlay visibility settings or the generic
display-mode advice as the cause without further evidence.

The owner relaunched Redunar and Stardew again at 14:15 on September 24 and
reported the same missing overlay/metrics. Steam again logged activation
claimed and the 64-bit interposer's wrong-ELF-class warning. The game process
then stopped about 31 seconds after launch, and the Redunar app process exited;
no capture directory or producer status remained. This confirms the launch
handoff repeats but does not identify whether the game's own process loaded the
interposer. The short run also does not establish a second five-minute timeout.

On September 24, the owner installed diagnostic RPM `0.1.3-3.local.fc44` and
relaunched Stardew through Redunar. Steam logged SDL discovery and hook-install
failure markers for several processes, with no presentation or producer hello.
However, review found these discovery markers can be false positives: probing
an arbitrary `dlopen` handle may resolve Redunar's own exported SDL wrapper, and
the diagnostic did not exclude it. The v2 log therefore does not prove that
Stardew's SDL hook failed. Stardew's installed executable is 64-bit and bundles
SDL2; both its bundled SDL2 and Steam Runtime SDL2 expose the expected x86_64
DynAPI stub layout. The producer is still not confirmed connected.

On September 24, the owner installed diagnostic RPM `0.1.3-4.local.fc44` and
relaunched Stardew. Its corrected markers show that earlier SDL “found” lines
resolved to Redunar's own wrapper in several launch processes; they do not
identify Stardew's SDL library. No SDL symbol lookup or presentation-hook call
was recorded. The installed sidecar matched the RPM payload (SHA-256
`dd1a7742e27091a7689e8023b5cedff68ecb0bd5981614f07df28136d83b8836`). The
owner also reports live click effects but no background music, so live audio is
only partially confirmed; Replay audio remains untested.

The owner installed diagnostic RPM `0.1.3-5.local.fc44` and reopened Stardew
through Redunar. The installed sidecar matched its RPM payload (SHA-256
`800cea4c7a1d993ac8b5d6aea644979c0980bffc6cdd1328e63b48818cd87c77`). Its
launch log confirms the preload constructor ran in the launch chain, but no
SDL-named `dlopen`, SDL symbol lookup, presentation hook, or producer hello was
recorded. This does not establish that Stardew made those calls through the
interposed entry points.

A source audit then found MonoGame's loader resolves `dlopen` and `dlsym` as
function pointers. Redunar's `dlsym` wrapper previously returned the system
loader's addresses unchanged for those names, allowing later SDL loads and
symbol lookups to bypass Redunar. RPM `0.1.3-6.local.fc44` was installed, but
packaging used an older sidecar from the Tauri release directory. Its running
session copied sidecar SHA-256
`625f23872ceb9009c904b3503070ebb80745091330372ee607328ee987653099`, which
does not contain the new loader-pointer markers. The screenshot from that run
still showed `0 frames received`; it did not test the source change.

The owner installed corrected RPM `0.1.3-7.local.fc44` and launched Stardew
through Redunar at 16:31 on September 24. The installed and session sidecars
match the RPM payload (SHA-256
`045ca7f6ce4cd571b3b36cced7bbabd21958cc34fcc922b92b572db1af004ae0`). The
game's loader-function lookups were intercepted, SDL loaded, presentation
hooks were installed and selected, and the producer sent its hello. Redunar
confirmed the producer after three exports and started Replay audio capture
from the default output monitor. The owner confirms in-game metrics and
Instant Replay work, and live sound effects are audible. Background music is
still silent; changing the in-game music toggle had no effect. The saved game
preferences show `startMuted=false`, `musicVolumeLevel=0.75`, and
`soundVolumeLevel=1`. This confirms the OpenGL metrics/overlay/Replay frame
path on this host, while Stardew's live music issue remains unresolved. Replay
clip audio playback has not been separately confirmed. The owner confirms
music plays from a direct Steam launch with the same game settings, making the
Redunar launch path the remaining difference. The saved Redunar catalog has
Gamescope and GameMode disabled for this game. The next discriminating check is
complete: the owner launched from Steam with only the installed OpenGL sidecar
added to `LD_PRELOAD`, preserving inherited preload entries, and reports the
music still plays. This rules out the sidecar by itself. The remaining useful
comparison is a Redunar launch whose only difference is that Replay's
host-side audio monitor is suppressed while video, metrics, and overlay stay
enabled; diagnostic RPM 0.1.3-8.local.fc44 provides exactly that (see
local-builds/replay-audio-monitor-disabled-diagnostic-2026-09-24/STATUS.txt).
Supporting evidence: the saved clip from the 16:33 silent-music session
(redunar-replay-1790260387-634017038-1.mp4) records the default output mix and
contains intermittent effects for about 21 s followed by continuous silence
from 20.9 s to 61.7 s at -45 dB, while a healthy ARC Raiders control clip has
no silence at that threshold; music never reached the system mix during the
Redunar run. Replay starts a host-side default-output recording stream after
its frame pipeline becomes active; the source does not explicitly mute or
reroute game playback, so this is a candidate to isolate, not a confirmed
cause. The 0.1.3-8 test required unmuting inside Stardew first because its
saved startup_preferences had startMuted=true from the 16:51 exit; the later
0.1.3-9 run started unmuted. Local RPMs are
diagnostics, not release builds; each
artifact's
`STATUS.txt` records its hash and collection details.

On September 24 the owner installed diagnostic RPM `0.1.3-8.local.fc44`, which
suppresses only Replay's host-side default-output audio recorder while keeping
Replay video, metrics, and overlay active, and launched Stardew through
Redunar. Music, metrics, and Replay all worked. That isolates the recorder as
the piece that disturbed Stardew's music during full Redunar sessions.
Discovery at the time preferred the PulseAudio monitor route on this PipeWire
host. Fix RPM `0.1.3-9.local.fc44` reverses that preference: native PipeWire
capture first, Pulse monitor as fallback. Its owner validation passed on
September 24: music, effects, metrics, and Replay all worked together in one
Redunar launch, and the saved clip redunar-replay-1790264709-515828288-1.mp4
carries 28.9 s of Opus audio with only short silence gaps, confirming the
native route records the mix. This closes the Stardew silent-music issue for
Redunar OpenGL launches on this host with build reference
redunar-app-0.1.3-9.local.fc44. See
local-builds/pipewire-preferred-audio-capture-2026-09-24/STATUS.txt.

Before further real-game acceptance, record the exact installed binary and
sidecar hashes, game PID and mapped audio libraries, effective audio route,
and whether the game creates a sink-input. Keep private paths and raw
diagnostics out of normal logs.

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
- [ ] Start with Show Redunar branding disabled and verify every overlay layout
      launches without the label while metrics remain visible. Toggle it on and
      off during the session and verify the live update does not restart the game.
- [ ] Confirm recording settings remain locked until the owned game closes,
      while visibility-only saves work. Availability comes from runtime status,
      not the saved preference alone.
- [ ] Confirm frame/encode counters progress and a healthy populated replay
      buffer exists. Record drops, overruns, resize/reset behavior, and failures.
- [ ] Verify Replay records the current default output and follows an output
      change during the session. Check controlled unrelated audio separately;
      mixed output can include other apps and is not game-only. See
      [REPLAY](../../REPLAY.md).
- [ ] Open/close the replay menu by shortcut with the main window visible and
      hidden. Verify transparent exterior, no desktop sidebar/black bars, correct
      focus, Escape, repeated shortcut dismissal, and outside-click preference.
      Closing the menu must not quit Redunar.
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
