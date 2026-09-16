# Redunar

Redunar provides instant replay recording and in-game metrics for Steam games
running on Linux with AMD hardware.

The current release targets x86_64 Linux with AMD/RADV graphics. It includes:

- Instant replay recording with a rolling buffer
- An in-game metrics overlay
- Session history with frame and hardware readings
- Clip trimming and playback in Redunar
- Steam game discovery and launch profiles

## Install

On a supported system, run:

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://redunar.com/install.sh | sh
```

The installer detects the Linux package format, verifies the signed release
manifest, and uses the system package manager. The current package boundary is
x86_64 glibc Linux in the Fedora/RHEL, Debian/Ubuntu, Arch, and openSUSE
families.

## Updates

Redunar checks signed GitHub release metadata from Settings. Manual checks are
available, and automatic checks run when the app starts by default. The native
process verifies the manifest and package checksum before handing a verified
package to the system package installer. Settings, clips, and history remain
in place across an update.

## Build and run locally

From the repository root:

```sh
python3 output/tauri-redunar/build-native.py
tools/run-tauri-release-local.sh
```

Run the release checks without installing:

```sh
tools/check-tauri-release.sh
```

To build signed installer assets, provide the release private key outside the
repository:

```sh
REDUNAR_RELEASE_SIGNING_KEY=/absolute/private.pem tools/build-install-assets.sh target/install-assets
```

## Documentation

- [Architecture](ARCHITECTURE.md) — components, lifecycle, persistence, and recovery
- [Replay](REPLAY.md) — recording, shortcuts, playback, and audio behavior
- [Hardware support](HARDWARE-SUPPORT.md) — implemented platform scope and limits
- [Testing](TESTING.md) — automated and manual verification
- [Packaging](packaging/README.md) — package formats and dependency obligations
- [Third-party notices](packaging/THIRD-PARTY-NOTICES.md) — dependency licenses and attribution
- [Tauri application guide](output/tauri-redunar/README.md) — native development entry points

## Scope

Redunar does not require an account or collect telemetry. Steam discovery and
artwork use files already present on the system. Capture and overlay features
are capability-gated and report when required hardware or media support is
unavailable.

This release makes a focused support claim: x86_64 Linux, AMD/RADV graphics,
and Steam games. Compatibility may vary by game and system configuration;
broader hardware, distribution, launcher, and game support is not claimed.

## License

Redunar is licensed under the [GNU General Public License v3 or later](LICENSE).
Third-party components retain their own licenses and notices; see
[THIRD-PARTY-NOTICES.md](packaging/THIRD-PARTY-NOTICES.md).
