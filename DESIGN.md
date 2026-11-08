# Redunar interface contract

This document defines the current Tauri interface. Source lives in
`output/tauri-redunar/ui/`; native behavior is documented in
`ARCHITECTURE.md`, `REPLAY.md`, and `HARDWARE-SUPPORT.md`.

## Shell and visual language

The main navigation contains Overview, Library, Global settings, Instant Replay,
History, and Settings. The expanded sidebar centers the Redunar wordmark; the
collapsed sidebar uses the Comet R mark. Every navigation control has an
accessible label and an explicit collapse or expand action. The sidebar control
keeps its panel frame and divider fixed; only its chevron moves and reverses
direction for the expanded or collapsed state.
The sidebar starts below the native titlebar, preserving the wordmark's top
inset in both windowed and maximized layouts.

Use a black canvas, charcoal panels, grey separators, and Redunar red
(`#e7474f`). Keep graphics subordinate to measurements and controls. Use the
bundled Noto Sans and Noto Sans Mono fonts. Preserve the shipped font notices.
Lucide artwork comes from the version indexed in `lucide-artwork.json`. Retain
the ISC and Feather MIT notices in `packaging/LICENSE-Lucide.txt`; icon motion
is implemented locally without a framework dependency.
Marketing language, idle animation, and session identity do not belong in
the application workspace.

The desktop workspace uses graphite surfaces, restrained red selection fills,
and clear type hierarchy. Key frame measurements share a flat row with quiet
separators. Keep session status compact and supporting
controls quiet. `workspace-theme.css` follows the component styles and changes
presentation only; it excludes the in-game Replay menu and preserves overlay
preview geometry, keyboard focus, disabled states, and draft actions.
Dropdowns use neutral graphite rows, white labels, and a red selection mark.
Secondary text, media controls, and slider tracks retain neutral grey values.

## Motion, focus, and scrolling

Motion is short and functional:

- dialogs fade in over 140 ms;
- dropdown surfaces unfold over 200 ms and fold away over 160 ms, moving at
  most 2 px toward their anchor. Closing releases input immediately; reversing
  keeps the painted frame. Scroll, resize, navigation, and dialog teardown
  remove the surface immediately;
- save bars fade and rise 4 px over 140 ms;
- sidebar and workspace offsets transition over 220 ms;
- new recordings and cards settle over 180–220 ms;
- thumbnails fade in over 160 ms;
- tab and option selection plates use a damped spring, preserving position
  and velocity when retargeted. Tab controls stay mounted during same-page
  renders, and nested panels do not replay entrance fades;
- Lucide control icons use stroke drawing or individual part motion over
  500–1000 ms on hover and keyboard focus. Overview uses Group, whose corner
  frame draws inward with the two blocks; Library Big tips its slanted book
  clockwise around the bottom edge. An active gesture finishes without
  restarting on internal pointer moves; reduced motion settles it immediately.
  The clip action row uses the same icon motion: Folder Open hinges its front
  flap, Trash separates its lid and bin vertically, and utility actions use
  bounded part or group movement. Playback, Save replay, and dropdown arrows
  remain still. Copy shows a check
  for three seconds after a successful clipboard write. Favorite uses a heart,
  a 0.97 press spring, and six small hearts from the button’s top center after
  successful addition to favorites. Bursts last 800ms with 50ms staggering;
  removing a favorite cancels remaining particles without starting a burst.
  Failed saves retain the acknowledged state.

Level sliders gently enlarge the thumb and reveal a soft red halo on hover.
Pressing slightly compresses the hovered thumb and darkens its center. Hover,
press and release settle over 120ms. The filled track ends at the thumb center.
Range values and fill positions follow input immediately, including dragging.
Time and trim controls retain their independent geometry and behavior.

Switches keep their native checkbox and existing hit area. Pressing gently compresses
the track and widens the handle; the handle slides over 260ms. Saved preferences
keep the same mounted control through acknowledgement and failures. A pending
save keeps its last saved position and a visible busy state.

History and microphone refresh actions show progress inside the button, followed
by a short check or failure indication. Duplicate actions do not start another read.
History refresh retains the selected session and list position. Microphone input
and Refresh inputs are visibly disabled while the microphone is off or a game
owns the setting. Input selection adds no changing description; the controls
retain an accessible explanation of their requirements. Errors and loading
remain separate.

Setting descriptions and action names stay stable during ordinary edits.
Selected values, validation errors, progress and saved-state indicators remain
accurate. Copy and refresh actions confirm completion through their icons and
accessible status messages without replacing their visible action names.

Reduced motion disables these effects. Selection feedback and dismissal remain
immediate. Reversing an action cancels stale completions and keeps the current
painted state.

Scrolling is owned by the WebKit engine. Do not intercept wheel events or add a
JavaScript scroll animation. Keep nested scroll areas, high-resolution input,
editing controls, focus, and scroll position native. Scrollbars use an inset
graphite thumb and a reserved gutter so controls remain visible.

Dialogs focus a neutral title or content target, preserve Escape and focus
containment, and restore focus to the invoking control. Pointer interaction
must not leave a false focus outline. Keyboard operation remains available for
every control.

Dialogs use the shared graphite surface, an icon-led heading, and a separated
action footer. The standard dialog keeps its 540px width and 28px inset; the
compact file form uses a 460px width and 24px inset. Each footer uses its shell's
padding so short dialogs have no scrollbars. Long content wraps and scrolls
vertically within the bounded dialog. Component styles defer to this shared
shell and footer. The footer keeps secondary dismissal actions quiet while the primary action uses
Redunar red. Discovery and settings forms reuse this shell. Clip copies use a 440px version
with the same heading and action footer, a shared filename/format row, and a
size field beside its label. A range switch appears only when both the entire
clip and a selection are available. Compression retains its original bounds,
validation, cancellation, and retry context.

Notifications use a bottom-right stack of at most three rounded graphite
toasts. Each rises from the edge on a damped spring and leaves over 200 ms.
When another toast remains, a departing card fades in place behind the stack;
only the last card slides toward the bottom edge. Hidden backing surfaces must
not cross below the active notification.
Older cards peek behind the latest; hover or keyboard focus unfolds the stack
and pauses its reading timers. Timers also pause while the window is in the
background. Repeated messages refresh one toast. Every toast has a dismiss
button and a remaining-time bar; errors use an alert announcement. Toasts
remain above draft actions and inside an open dialog's top layer. Reduced
motion settles the stack immediately.

Settings places Software updates first, followed by a Preferences card with
the same plain rows, short descriptions, right-aligned toggles, and quiet
separators as Session behavior. Debug logging omits the off status and its
empty space; restart instructions, active status, and failures remain visible.

## Shared controls and layout

- Secondary buttons use quiet grey fills; primary buttons use a muted red fill
  and a brighter border. Hover and press change the fill over 150 ms, without
  glow, shadow, or glyph movement. Destructive actions remain explicit.
- Toggles use a graphite track and solid light handle. Enabled tracks use muted
  red; handles slide over 180ms with clear disabled, hover, press, and keyboard
  focus states. Auto-saved labels share the Overlay & metrics 4px corner radius.
- Sliders keep readable values, units, bounds, keyboard movement, and disabled
  states. Settings sliders share a wide control column and fill the row on
  narrow windows. Playback volume grows within the player controls; timelines
  and export sliders fill their available width. Percentage and microphone-level
  sliders use 5px tracks, 18px handles, and 44px input areas. Exact-value fields
  commit on Enter or blur, revert unfinished edits on Escape, and respect the
  original bounds and steps. Pointer values update immediately; only hover and
  press feedback animate. Timeline and trimming geometry stays independent.
- Short fixed choices use the same rounded switcher as Layout, with a moving
  graphite selection plate and no red underline. This includes Position,
  information presets, recording mode, quality, formats, and game overrides. Palette colors use the
  same frame and selection motion. Arrow keys, Home, End, focus restoration,
  and disabled states remain available; select-backed switchers retain typeahead.
  Native selects retain the form values and existing change/save behavior.
  Filters, sorting, game lists, microphone inputs, session comparisons, and
  the eight replay durations keep dropdowns with keyboard, dismissal, and
  viewport placement.
- Avoid hover tooltips for controls with visible labels. Keep a native title only
  where an icon-only action needs a plain-language cue.
- Keep at least 12 px between buttons, 16 px around standalone settings rows,
  and 24 px around save and discard areas.
- Page headers expose relevant actions without a page title or description.
  Keep supporting explanations beside the controls they explain, and keep page
  names available through navigation and the document title.
- Library and History catalog footers share full-width separators, 16px space
  above the note, 12px text with 20px line height, and a first-line information
  icon.
- Tab groups use a rounded frame with an inset graphite selection and a red
  accent. Keep the same padding across page and editor tabs.
- Show Save/Discard only when drafts exist. Identify whether the draft belongs
  to shortcuts, global settings, or a game profile.

## Overview

Give the frame graph more space than the active-session banner. Keep the timer
beside End session and show session settings, saved clips, and feature choices
in a separate status area. Use Recent captures and System metrics as labels.
Replay status shows seconds buffered and audio activity.

Group CPU and GPU load, temperatures, RAM, and VRAM when available. Missing
readings remain unavailable and retain their row height so monitor interruptions
do not move the scrollbar. Overview shows Add your first game only after a
successful empty catalog load; loading and failed reads do not look empty.

## Library and Steam

Keep the searchable game catalog beside selected-game details. The catalog has
the same quiet border, 16px corners, and inset spacing as History. It has its
own bounded scroll area and never extends below the details panel on
desktop. Narrow layouts place the catalog above the details. Preserve game,
profile, and cached artwork after uninstall; use neutral fallbacks for missing
artwork.

Game settings and Game detection are separate tabs. Show inherited values,
custom values, Use global settings, and independent drafts. Preserve the
selected game, artwork, search, Steam setup, and drafts while switching tabs.

Every selected Steam game has a Play from Steam panel with one paste
instruction, the exact launch option, Copy, and a short startup explanation.
Setup confirmation does not imply capture readiness. Ambiguous and failed
setup states retain an explanation and Check again.

Automatic startup may keep Redunar hidden in the tray. Opening Redunar removes
the temporary icon without changing Close to tray. If Close to tray is off, an
automatically started instance exits after game exit and successful cleanup
when the window was never used. Cleanup failures show the window for attention.

## History

A searchable session browser stays beside the selected session on desktop.
Search, game, and date filters limit the list. The session summary, timeline,
and selected readings use separate rounded graphite cards. Summary values use
one flat row, while selected readings are grouped into frame, CPU, and GPU values.

The graph preserves recorded points, gaps, scales, red traces, and area fills.
Quiet grid lines and compact axis labels keep attention on the data. The timeline
and its wide slider stay aligned, including during keyboard input. The graph
switcher uses Overview's compact control size. Comparison
requires an explicit session from the same game. Older sessions retain their
frame-data explanation and missing-reading states. Narrow windows stack the
browser above the detail cards.

## Overview settings and overlay

Global settings own frame metrics, overlay visibility and appearance, recording
configuration, replay folder and menu preferences, and optional shortcuts.
Per-game settings override the supported subset with explicit inheritance.
Show overlay uses the settings draft and Save/Discard both when idle and
during a session. Changing the toggle updates the preview; saving applies the
visibility to a supported running overlay while respecting per-game overrides.

The overlay preview uses the same geometry, metric order, palette roles, and
dimensions as the in-game renderer. The sample stays centered and scales down
to fit the preview when needed. Position controls the saved in-game corner.
Palette colors remain visible inside the sample while the surrounding controls
retain graphite surfaces. Overlay & metrics follows the shared settings surfaces
used by the other global tabs. The preview and runtime
controls share one card, with two aligned session rows beside the preview.
A separate full-width card contains the content-sized Appearance and Metrics
tabs and their controls. Appearance uses one setting per row with labels on the
left and controls aligned on the right. Metrics groups frame and hardware
measurements into two lists. Narrow windows stack the sections and controls.
Supported palettes are Redunar, Glacier, Ember, Mint, Mono, Amethyst, Solar,
and Rose.

Layout choices use a small content-sized rounded switcher matching Overview,
with a moving graphite selection plate and no red underline. Arrow keys, Home,
and End select a layout.

The information presets use the shared small switcher: FPS only, Compact,
Detailed, and Custom. Metric rows use the shared toggles, enabled for Custom. Grid,
Ribbon, and Telemetry independently control layout. FPS only disables layout
and branding controls while retaining their saved values. Custom metrics remain
valid and non-empty.

Automatic replay has no global or per-game Off selector. Recording and quality
settings are capability-gated and remain locked through launch, recording, and
teardown. A visibility-only change must not be blocked by recording state.

## Instant Replay and audio

The clip rail ends with a slim capacity bar and free-space label. Whole-drive
usage and recording size remain available in accessible details; low-space and
exceeded-budget warnings remain visible when applicable. Playback and trimming
share one filmstrip scrubber so the player does not present a second seek track.
The selected clip has five direct 40px icon actions: Edit filename, Favorite,
Compress, Open externally, and Move to trash bin. Existing confirmations and
export behavior apply. The playback percentage field sits beside the transport
buttons, before the speaker and volume slider; Play, Back, and Forward share
its 40px height. Rail cards enlarge by 1.2% around their center on hover without
shifting position.

The empty player uses one bounded graphite surface, the Replay icon, and short
guidance. Loading and inventory failures stay distinct from an empty clip list;
failures retain their technical details and offer a retry.

Inactive Replay guidance explains that a supported game must be started.
Recording mode supports 60 FPS and Variable FPS. Variable mode follows
accepted game presents within hardware and resource limits; it is not a
display-scanout measurement.

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
