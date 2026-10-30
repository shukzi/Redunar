# Redunar native app

The Tauri application connects one app-lifetime `RedunarService::for_tauri()`
to the shared Rust backend. The native host owns commands, supervision, media
workers, tray integration, and windows.

## Guides

- [Architecture](../../ARCHITECTURE.md)
- [Interface design](../../DESIGN.md)
- [Replay and media](../../REPLAY.md)
- [Hardware support](../../HARDWARE-SUPPORT.md)
- [Testing](../../TESTING.md)
- [History calculations](HISTORY-CALCULATIONS.md)
- [Packaging](../../packaging/README.md)

## Build and inspect

Run commands from the repository root:

```sh
python3 output/tauri-redunar/build-native.py
node --test output/tauri-redunar/tests/*.test.mjs
cargo test --offline --manifest-path output/tauri-redunar/src-tauri/Cargo.toml --all-targets
cargo clippy --offline --manifest-path output/tauri-redunar/src-tauri/Cargo.toml --all-targets -- -D warnings
```

Use `tools/check-tauri-release.sh` for the complete native release gate.
Builds use checkout-owned `.redunar-build` targets and do not install the app.
Installation and restart are separate actions.

Release builds use the signed GitHub release channel:
`https://github.com/shukzi/Redunar/releases/latest/download`.
`REDUNAR_UPDATE_SOURCE_URL` can point an isolated test to another HTTPS feed.

## Runtime boundaries

- Native data supplies Library, profiles, history, clips, and runtime status.
- Drafts are separate from saved values; saves preserve inheritance and reject
  stale changes.
- Supported launches prepare metrics and request Replay through capability
  checks. The overlay remains reversible while the runtime is prepared.
- End session releases Redunar resources without killing the game. Quit cleans
  up the session and exits.
- The Replay menu is rendered inside the captured game; the in-app preview is a
  configuration surface only.
- The service exposes no arbitrary privileged commands and never runs the UI as
  root.

## Verification

Run the automated fixtures and native probes in [TESTING.md](../../TESTING.md).
Use [REAL-GAME-TESTING.md](../../REAL-GAME-TESTING.md) for a supported native
Steam run. A browser or synthetic pass does not establish codec, GPU,
compositor, or real-game support.
