# Real-game validation

Use this procedure for a supported native Steam game after the exact build and
package have passed automated checks. Real-game runs are separate from fake
hardware, browser, and fixture tests.

## Before the run

Record the build or package identifier, source commit, distribution, desktop
session, GPU and driver, game, and test date. If using an installed package,
compare the installed executable, sidecars, helpers, resources, and permissions
with that exact package. A build or package alone does not prove installation
or a fresh process.

Use one Redunar instance and one session coordinator. Do not run a game test against
an already-owned live coordinator. Keep recordings and existing user data
outside disposable test state.

## Launch and capture

1. Launch Redunar and confirm the game is detected with the intended profile.
2. Launch the game through the supported native Steam path.
3. Confirm the overlay and measurements show actual data or an honest
   unavailable state.
4. Start Instant Replay, wait for a buffer, and save a short clip.
5. Check the saved file, audio result, playback, trimming, export, and history.
6. Exercise the configured shortcuts, window visibility, and any affected
   settings.
7. End the game normally, then confirm session cleanup, process exit, and
   resource release.

Record failures and missing capabilities with the exact build and environment.
Do not generalize a single game or GPU result to all Linux systems.

## Audio and performance

Redunar captures the mixed system output and may include other applications.
Do not describe it as game-only audio. Record the selected output and audio
backend when audio matters.

Use `PERFORMANCE.md` for collection budgets and measurement procedures. Record
measured overhead, frame pacing, drops, CPU/GPU use, and memory when the test
covers performance. Synthetic results and successful startup do not establish
real-game acceptance.

## Isolated runners

After a production build, these finite local runners exercise isolated Vulkan
session and Replay paths:

```sh
tools/run-tauri-vulkan-session-acceptance.sh
tools/run-tauri-vulkan-replay-acceptance.sh
```

They require a working local Vulkan display and, for Replay, a supported
hardware encoder. They do not replace a real-game run or qualify Flatpak Steam.
