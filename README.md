<p align="center">
  <img src="assets/branding/redunar-logo-256.png" width="96" height="96" alt="Redunar app icon">
</p>

<h1 align="center">Redunar</h1>

<p align="center"><strong>Instant replay and in-game metrics for Linux.</strong></p>

<p align="center">Save recent gameplay, trim your clips, and track performance.</p>

<p align="center">
  <a href="https://github.com/shukzi/Redunar/releases/latest"><img src="https://img.shields.io/github/v/release/shukzi/Redunar?label=release&amp;color=b93646" alt="Latest release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0%2B-647379" alt="License: GPL-3.0-or-later"></a>
</p>

<p align="center">
  <a href="https://github.com/shukzi/Redunar/releases/latest">Download</a> ·
  <a href="#documentation">Documentation</a> ·
  <a href="https://discord.gg/n5UkDEMQMX">Discord</a> ·
  <a href="https://github.com/shukzi/Redunar/issues/new/choose">Report a bug</a>
</p>

## Features

- **Instant Replay** — save recent gameplay from a rolling buffer.
- **In-game overlay** — view FPS, frame times, and system metrics. Customize colors, layout, and detail.
- **In-app playback** — play back, trim, compress, and export clips.
- **Session history** — compare frame pacing, temperatures, and load across each session.
- **Game profiles** — use global defaults or customize settings for individual games.

## Install

For x86_64 Linux with compatible AMD/RADV or NVIDIA graphics and native Steam.
See [hardware support](HARDWARE-SUPPORT.md) for requirements and current limitations.

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://redunar.com/install.sh | sh
```

The installer selects your distribution’s package format, verifies the
signed release manifest, and installs through your system package manager.

With Wget:

```sh
wget --https-only -qO- https://redunar.com/install.sh | sh
```

## Getting started

For a native Steam game:

1. Open Redunar and scan for installed games in Library.
2. Copy its Launch Options from **Play from Steam**.
3. Paste them into Steam’s **Properties → General → Launch Options**.
4. Launch the game from Steam or through Redunar.

Replay and metrics depend on your GPU, game, and driver.
Flatpak Steam capture is currently unsupported.

## Documentation

- [Replay](REPLAY.md) — recording, shortcuts, playback, and audio.
- [Hardware support](HARDWARE-SUPPORT.md) — requirements and compatibility.
- [Development](CONTRIBUTING.md) — building, contributing, and local checks.
- [Architecture](ARCHITECTURE.md) — how Redunar works.

## Feedback

Join the [Discord community](https://discord.gg/n5UkDEMQMX) for questions, ideas, and shared setups.

Bug reports and feature suggestions are welcome.
Use the [issue forms](https://github.com/shukzi/Redunar/issues/new/choose).

Contributions to the official repository are invitation-only.
See [CONTRIBUTING.md](CONTRIBUTING.md) for details.

## License

Redunar is licensed under [GPL-3.0-or-later](LICENSE).

See [third-party notices](packaging/THIRD-PARTY-NOTICES.md)
for dependency licenses and the [branding policy](TRADEMARKS.md)
for use of the Redunar name and logo.
