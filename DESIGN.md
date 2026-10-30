# Redunar interface contract

This document defines the current Tauri interface. Source lives in
`output/tauri-redunar/ui/`; native behavior is documented in
`ARCHITECTURE.md`, `REPLAY.md`, and `HARDWARE-SUPPORT.md`.

## Shell and visual language

The main navigation contains Overview, Library, Global settings, Instant Replay,
History, and Settings. The expanded sidebar centers the Redunar wordmark; the
collapsed sidebar uses the Comet R mark. Every navigation control has an
accessible label and an explicit collapse or expand action.

Use a black canvas, charcoal panels, grey separators, and Redunar red
(`#e7474f`). Keep graphics subordinate to measurements and controls. Use the
bundled Noto Sans and Noto Sans Mono fonts. Preserve the shipped font notices.
Marketing language, decorative motion, and session identity do not belong in
the application workspace.

## Motion, focus, and scrolling

Motion is short and functional:

- dialogs fade in over 140 ms;
- dropdowns move at most 2 px and enter within 100 ms;
- save bars fade and rise 4 px over 140 ms;
- sidebar and workspace offsets transition over 220 ms;
- new recordings and cards settle over 180–220 ms;
- thumbnails fade in over 160 ms.

Reduced motion disables these effects. Selection feedback and dismissal remain
immediate. Reversing an action cancels stale completions and keeps the current
painted state.

Scrolling is owned by the WebKit engine. Do not intercept wheel events or add a
JavaScript scroll animation. Keep nested scroll areas, high-resolution input,
editing controls, focus, and scroll position native. Scrollbars use an inset
red thumb and a reserved gutter so controls remain visible.

Dialogs focus a neutral title or content target, preserve Escape and focus
containment, and restore focus to the invoking control. Pointer interaction
must not leave a false focus outline. Keyboard operation remains available for
every control.

## Shared controls and layout

- Secondary buttons use quiet grey fills; primary buttons use a muted red fill
  and a brighter border. Destructive actions remain explicit.
- Toggles use an outlined track and ring handle with distinct enabled,
  disabled, hover, and keyboard-focus states.
- Sliders keep readable values, units, bounds, keyboard movement, and disabled
  states.
- Selectors retain native keyboard, typeahead, dismissal, focus restoration,
  and viewport placement behavior.
- Keep at least 12 px between buttons, 16 px around standalone settings rows,
  and 24 px around save and discard areas.
- Show Save/Discard only when drafts exist. Identify whether the draft belongs
  to shortcuts, global settings, or a game profile.

## Overview

Give the frame graph more space than the active-session banner. Keep the timer
beside End session and show profile, saved captures, and enabled features in a
separate status area. Use Recent captures and System metrics as labels.

Group CPU and GPU load, temperatures, RAM, and VRAM when available. Missing
readings remain unavailable and retain their row height so monitor interruptions
do not move the scrollbar. Overview shows Add your first game only after a
successful empty catalog load; loading and failed reads do not look empty.

## Library and Steam

Keep the searchable game catalog beside selected-game details. The catalog has
its own bounded scroll area and never extends below the details panel on
desktop. Narrow layouts place the catalog above the details. Preserve game,
profile, and cached artwork after uninstall; use neutral fallbacks for missing
artwork.

Game settings and launch matching are separate tabs. Show inherited values,
custom values, Reset to global settings, and independent drafts. Preserve the
selected game, artwork, search, Steam setup, and drafts while switching tabs.

Every selected Steam game has a Play from Steam panel with one paste
instruction, the exact launch option, Copy, and a short startup explanation.
Setup confirmation does not imply capture readiness. Ambiguous and failed
setup states retain an explanation and Check again.

Automatic startup may keep Redunar hidden in the tray. Opening Redunar removes
the temporary icon without changing Close to tray. If Close to tray is off, an
automatically started instance exits after game exit and successful cleanup
when the window was never used. Cleanup failures show the window for attention.

## Overview settings and overlay

Global settings own frame metrics, overlay visibility and appearance, recording
configuration, replay folder and menu preferences, and optional shortcuts.
Per-game settings override the supported subset with explicit inheritance.

The overlay preview uses the same geometry, metric order, palette roles, and
dimensions as the in-game renderer. Appearance and Metrics are mounted editor
tabs beside the preview. Supported palettes are Redunar, Glacier, Ember, Mint,
Mono, Amethyst, Solar, and Rose.

The information presets are FPS only, Compact, Detailed, and Custom. Grid,
Ribbon, and Telemetry independently control layout. FPS only disables layout
and branding controls while retaining their saved values. Custom metrics remain
valid and non-empty.

Automatic replay has no global or per-game Off selector. Recording and quality
settings are capability-gated and remain locked through launch, recording, and
teardown. A visibility-only change must not be blocked by recording state.

## Instant Replay and audio

Inactive Replay guidance says “Launch a supported game to build a local
buffer.” Recording mode supports 60 FPS and Variable FPS. Older fixed 30 and
120 FPS profiles remain readable. Variable mode follows accepted game presents
within hardware and resource limits; it is not a display-scanout measurement.

The MKV/MP4 choice applies to future saves. Menu and save shortcuts may be
cleared. The microphone toggle and recording level are separate from system
voice-chat volume. A microphone test lasts six seconds, requires explicit
activation, and remains temporary. Stop, clear, navigation, focus loss, game
launch, and shutdown cancel its capture session.

Input discovery starts when the settings tab opens and keeps controls disabled
until it completes. Refreshing inputs must preserve unrelated drafts. Missing
input and discovery errors use a separate feedback row.

## Session lifecycle and safety

Closing the window during a live game returns it to the tray and preserves
Replay and session ownership. Explicit Quit ends the Redunar session without
ending the game. Launch arguments are validated as literal arguments with an
explicit working directory; never assemble a shell command from user input.

The webview requests actions from native services. It does not own permissions,
recovery, arbitrary paths, or privileged operations. Native services own
session and profile state, startup and shutdown, capability checks, cleanup,
and error recovery.

## Accessibility and validation

Keep text readable during motion, with reduced motion, while a page is hidden,
and after a failed resource load. Preserve labels, keyboard focus, Escape,
focus restoration, native control semantics, and visible unavailable states.

For UI changes, inspect the effective stylesheet order and run the production
Tauri build plus the relevant UI checks. Verify the affected navigation,
drafts, startup and shutdown, unsupported hardware, errors, cancellation, and
cleanup paths. Do not claim hardware or real-game support from synthetic data.
