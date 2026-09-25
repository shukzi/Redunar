# Redunar interface contract

Current Tauri design, reconciled September 20, 2026. This file owns approved
visual and interaction rules. Source is in `output/tauri-redunar/ui/`.

## Shell and visual language

Navigation is Overview, Library, Global settings, Instant Replay, History, and
Settings. Use Redunar's Comet R and wordmark, a compact sidebar, a quiet
breadcrumb header, and page titles in the workspace. Keep graphics subordinate
to measurements and controls. Keep marketing language, decorative motion, and
session-identity concepts out of the app workspace.

Use a black canvas, neutral charcoal panels, grey separators, and Redunar red
`#e7474f`. Avoid purple-tinted neutral surfaces and orange replacements for red.
The approved control colors remain independent of surface refinements. Inspect
the complete stylesheet order in `ui/index.html`; `app.css` alone is not the
rendered design. `workspace.css`, `controls.css`, the Precision styles, and the
scoped feedback/history/replay files refine the base.

Use the bundled Noto Sans for interface text and Noto Sans Mono for measurements
and timecodes. Preserve the shipped font notices. Icons retain a square aspect
ratio and must not shrink into squeezed shapes.

## Shared controls and spacing

- **Soft fill buttons:** quiet grey secondary surfaces, muted red primary fill
  with a brighter border, rounded corners. Keep destructive actions explicit.
- **Soft ring toggles:** outlined track and ring handle, with a larger hit area.
  Show enabled, disabled, hover, and keyboard-focus states distinctly.
- **Precision sliders:** thin filled track and outlined handle. Values, units,
  bounds, keyboard movement, and disabled state must remain readable.
- **Precision selectors:** reuse the shared native-select enhancement and its
  keyboard/typeahead, dismissal, focus restoration, and viewport placement.
- Keep at least 12 px between buttons and 16 px above/below standalone settings
  action rows. Preserve spacing when rows wrap. Supporting block text clears
  preceding components by at least 16 px, and save/discard areas by 24 px.
- Inputs and adjacent file-picker buttons align vertically. Text and controls
  must not touch dividers, card edges, or the component above them.
- Save/Discard appears only when there are unsaved changes, at the bottom of
  the visible workspace. Reserve space so it cannot cover content. Do not render
  an always-visible All changes saved bar or a duplicate inside Library.
- Dialog opening focuses a neutral dialog/title target rather than highlighting
  Close as the default action. Preserve accessible keyboard focus, Escape,
  focus containment, and restoration to the invoking control.

## Overview

Give the frame graph more space than the active-session banner. The chosen
session layout has a status footer: keep the timer beside End session and
separate profile, saved captures, and enabled-feature information. Do not show
Previous settings captured. No separate overlay-visibility action belongs here;
Global and per-game settings own that control.

System metrics groups CPU/GPU load, separate temperature meters, RAM, and VRAM
where available, with readable separators. Red shades follow the graph; missing
readings remain unavailable. Use Recent captures, not Recent moments, and
System metrics, not Under the hood.

## Library

Keep the compact searchable game catalog beside selected-game details. Use
poster artwork in the catalog and landscape banner artwork in the header;
never stretch a poster into a banner. Missing artwork has a neutral fallback.
Retain the game, profile, and cached artwork after uninstall. Disable Launch
game with small Not installed text underneath when the installation is missing.

Game settings and Launch matching are separate tabs. Clearly show inherited
versus custom values and Reset to global settings. Changes retain their own
drafts through navigation and unrelated saves. Installed-game import reviews
local Steam/XDG entries; running helper processes are not import candidates.
Executable, literal arguments (one per line), and working directory use the
native validation flow. Do not turn launch arguments into a shell command.

## Global settings and Settings

Global settings owns frame-metric collection, overlay visibility/appearance,
recording configuration, replay folder/menu preferences, and optional shortcuts.
Per-game controls override the supported subset with explicit inheritance.
Saving overlay visibility also updates a prepared running game according to
its inherited/custom setting; hiding must not remove the runtime.

Overlay preview reflects the real renderer's layout, metric order, scale, and
background-only opacity over a soft gradient. Compact, FPS only, Detailed, and
Custom previews show their actual geometry. Custom selections remain valid and
nonempty. Grid, Ribbon, and Telemetry change the structure independently of the
information preset and Custom metric selection. The bounded palettes are
Redunar, Glacier, Ember, Mint, Mono, Amethyst, Solar, and Rose. The preview must
use the same palette roles, rounded panel bounds, and layout dimensions as the
in-game renderer. Redunar branding is an optional global appearance setting;
its saved value updates a prepared running overlay through the same live path
as palette and layout changes. Renderer ABI details belong in
architecture/component documentation.

Automatic replay has no global/per-game Off selector. Frame-rate and quality
configuration is locked until the owned game closes; this must not block an
otherwise valid visibility-only save. The MKV/MP4 save container may change
during a session and applies to future saves. Menu/save shortcuts may all be
cleared.

Settings contains the current application preferences, including Close to tray
and software updates. Automatic update checks default on; the user can disable
them or request a manual check. Automatic checks do not install packages.
Close to tray, Beta access, and Debug log share one Preferences card with
consistent switch rows; software update controls remain together beside it or
below it when space is narrow.
When a verified update package is already cached, an automatic check keeps it
available. A manual check refreshes the signed release channel so a newer
version can replace that pending package; an unavailable channel leaves the
verified cached package available with an honest status.
When a startup check finds an update, direct the user to Settings to install it;
keep package-verification details in the Settings status. Installation always
requires a clear user action and remains native-owned. Update now requests
system authorization through polkit and installs the signed package through
the system package manager. Cancellation keeps the verified package ready to
retry. Settings reports completion only after the installed package version
is confirmed and says when a restart is needed.
Close to tray immediately controls tray icon visibility and closing behavior.
Keep this page limited to current user-configurable behavior and runtime
diagnostics.

Settings also owns the optional Debug log switch. It is off by default and
takes effect on the next Redunar start. When enabled, Redunar records its
operational messages, timestamped, into a private bounded file in the state
directory, rotating to at most two bounded files. The settings copy states
plainly that the file can include game names and session details, and offers
an Open log folder action once the file exists. The log never contains clip
media. Sharing it is always the owner's explicit choice.

Settings also offers Beta access, off by default. Anyone may opt in to early
features included in the installed build; it does not fetch code, change the
update channel, enable Debug log, or override a feature's native capability
checks. The preference is owned by the service and applies after an app restart
and a new game launch. In this build it admits experimental NVIDIA GPU metrics
and a single-GPU NVIDIA Vulkan Video Replay candidate. Hardware and codec
readiness still determine availability; the NVIDIA paths have not been verified
on NVIDIA hardware.

## Instant Replay

Use a player/editor beside an independently scrolling saved-clip rail. Selection,
loading, retry, deletion, and inventory refresh must preserve appropriate scroll,
search, focus, and save-duration state. Long filenames wrap or truncate without
forcing horizontal scrolling; show available game, duration, date, and size.

The video area's aspect ratio follows the decoded clip. Keep the entire image
visible without cropping, rounded boundaries, and playback controls directly
underneath. Do not crop video just to remove letterboxing; black pixels recorded
inside the file are part of the footage. Keep technical video details at top
right and a smaller filename above the game name at bottom left. No Storyboard
preview caption, fictional scene title, or expand control belongs in production.

The filmstrip has a time ruler, In/Out handles, playhead, selected duration, and
Export selection. The export dialog shows In, Out, and Duration in separated
columns and explains that the original remains. Saving/exporting reports success
only after native completion; loading failures offer a useful retry/error state.

The shortcut-opened Replay menu is rendered inside the game. No desktop or in-app
preview is exposed because it cannot represent the production Vulkan presentation.
See REPLAY for compositor limitations.
Successful in-game saves use the bottom-left Moment saved pill with saved length
and Local library, even when the metrics display is hidden.
An unavailable Replay state uses one stable, actionable reason from native
capture or encoder status; an inactive recorder never implies a populated buffer.

The in-game metrics overlay and Replay menu follow the approved modern
reference: soft rounded panels, quiet letter-spaced labels above bright
values, thin 1 px dividers, a dark-red selected duration cell with a red
underline, and a red-outlined save button. The eight bounded palettes keep
their accent, muted, text, divider, and panel roles in both the injected
renderer and this preview; the Replay menu keeps fixed Redunar control colors
independent of the metric palette. Renderer geometry and palette facts live in
the capture crates and the shader notes under
`crates/redunar-capture-vulkan/src/shaders/README.md`.

## History and honest data

Preserve the native report layout with the approved sidebar/search. Graph labels,
traces, slider, cursor, and selected readings must share coordinates and real
recorded timestamps. Use Redunar red for CPU and lighter red `#ff9297` for GPU
traces and legend swatches; keep measurement-card text neutral. This rule is
scoped to History, not an instruction to recolor unrelated hardware UI.

Use the [calculation contract](output/tauri-redunar/HISTORY-CALCULATIONS.md).
Never smooth away recorded spikes, invent missing samples, or report a value
that contradicts its plotted height. Empty, partial, unavailable, and
compatibility data need honest states. A fixture verifies presentation, not
native measurement.
