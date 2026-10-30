# Real-game validation

Current Tauri procedure, reviewed September 13, 2026. These are explicit
owner-controlled runs, separate from fake-hardware and browser fixture tests.
Use only a game/test mode whose capture and overlay policy is approved.

## Scope and prerequisites

The initial Steam scope is ARC Raiders and PEAK on the validated Fedora/AMD
development host. Retained records cover owner runs with both. Counter-Strike 2
and another Linux/WebKit environment are deferred from the initial release; do
not describe them as supported evidence or launch incidental multiplayer tests.
A new runtime change may require repeating an affected previously tested flow.

Build the active Tauri release, verify installed binary and sidecars with
`tools/check-installed-tauri-runtime.sh` when using the installed app, and fully
quit/reopen it. Only one instance may own the live coordinator. Keep build,
distribution, desktop/compositor, kernel, GPU/driver, mode, and test dates with
results. A package build is not proof of installation or a fresh process.

Use the current
[real-game acceptance checklist](output/tauri-redunar/REAL-GAME-ACCEPTANCE.md)
for launch, visibility, shortcuts, capture, replay, history, and cleanup.

## Deferred performance and audio qualification

Representative overhead, long-session measurements, and additional audio
source/provider validation are deferred from the initial release. Keep the
existing bounded probes and document the actual output-monitor fallback; do not
claim game-only audio isolation or a completed performance study.

For audio, verify the active default output and change outputs during the run.
The implementation captures the mixed system output and can include other
applications; do not claim game-only isolation. Use controlled nonsensitive
sounds and record the selected backend, output, and mixed-audio result.
[REPLAY.md](REPLAY.md) defines the behavior.

## Optional isolated diagnostics

After a production build, these existing runners exercise finite local Vulkan
fixtures and isolate their session/replay state:

```sh
tools/run-tauri-vulkan-session-acceptance.sh
tools/run-tauri-vulkan-replay-acceptance.sh
```

They need a working local Vulkan display and, for replay, a supported hardware
encoder. Read the selected runner before use; neither is a blanket instruction
to execute ignored tests or open real games.

Read-only corpus helpers are `tools/check-local-replay-codecs.sh` and
`tools/check-flatpak-replay-codecs.sh`. They test decode/seek with available
local runtimes; they do not establish a Flatpak game-capture bridge or another
host's WebKit integration. See [TESTING.md](TESTING.md) for the isolated native
WebKit playback probe and ordinary regression checks.

Only current Tauri controls and runtime behavior are acceptance requirements.
