# Redunar

Redunar provides instant replay recording and in-game metrics for Steam games
running on Linux with AMD hardware.
Games have local identities with global defaults and per-game overrides.

The active desktop application is **Tauri**, under
[`output/tauri-redunar/`](output/tauri-redunar/README.md), using the shared Rust
services in `crates/`. Initial validation targets x86_64 Linux with AMD
hardware. See [hardware support](HARDWARE-SUPPORT.md) for limitations.

## Build and run locally

Run from the repository root:

```sh
python3 output/tauri-redunar/build-native.py
tools/run-tauri-release-local.sh
```

The runner uses the release binary and its matching local capture/shortcut
components. It runs the real application; use isolated tests when production
state must not be touched. Do not run a second app against the same live state.

To verify and package without installing:

```sh
tools/check-tauri-release.sh
tools/build-linux-release.sh
tools/build-local-tauri-rpm.sh target/packages
REDUNAR_RELEASE_SIGNING_KEY=/absolute/private.pem tools/build-install-assets.sh target/install-assets
```

The stable local artifact is `target/packages/redunar-app.rpm`. Building it does
not install it or replace an already-running process. After an owner-controlled
installation, run `tools/check-installed-tauri-runtime.sh` and fully quit/reopen
Redunar, including any tray instance, before testing the updated build.
The installed executable is `/usr/bin/redunar-tauri`.

## Updates

The native update boundary now has a signed metadata checker. It validates the
release manifest, rejects malformed entries and unsafe sources, verifies the
embedded Redunar signature, compares the signed version with the running build,
selects the package for the supported Linux family, downloads it with a size
limit, and verifies its SHA-256 bytes before reporting it as available. The
Settings webview exposes the manual check and the default-on automatic-update
preference; it never installs packages or runs arbitrary commands.

The package manager remains the installation mechanism underneath the updater.
Users should also be able to rerun the signed installer or install a downloaded
package manually. User data must remain in place while application files are
replaced. Verified packages are retained in the user's private cache and can be
handed to the desktop package installer from Settings. Authorization,
installation result handling, restart, rollback, and release-source automation
remain before-publication work.

Release builds produced by `output/tauri-redunar/build-native.py` embed the
HTTPS GitHub release channel at
`https://github.com/shukzi/Redunar/releases/latest/download`. A
`REDUNAR_UPDATE_SOURCE_URL` environment value overrides that endpoint for an
isolated test feed. Development and test builds without either value keep the
explicit “Signed update checking is not configured in this build yet” status;
they do not contact a release service.

## One-command installer preparation

[`install.sh`](install.sh) is the noninteractive installer intended for the
website's eventual
`curl --proto '=https' --tlsv1.2 -fsSL https://redunar.com/install.sh | sh`
command. It
detects supported distribution families, rejects incompatible architecture,
glibc, musl, and immutable layouts, downloads the matching GitHub release asset,
authenticates the signed SHA-256 manifest, verifies the package bytes, and then
uses the distribution's native package manager.

The public GitHub repository is `shukzi/Redunar`. Render the publishable
installer without editing the template by hand:

```sh
tools/render-public-installer.sh shukzi/Redunar /absolute/output/install.sh packaging/release-signing-public.pem
```

`tools/build-install-assets.sh` first rebuilds every executable in the pinned
Debian 12/glibc 2.36 environment, then prepares Fedora and openSUSE RPMs, a DEB,
an Arch package, the portable archive, checksums, and the signed manifest expected
by the installer. Building or rendering these files does not publish, install,
or authorize a release. See
[packaging](packaging/README.md) for the current compatibility boundary.

## Current documentation

| Topic | Owner |
| --- | --- |
| Approved UI and interaction rules | [DESIGN.md](DESIGN.md) |
| Components, lifecycle, persistence, and recovery | [ARCHITECTURE.md](ARCHITECTURE.md) |
| Recording, shortcuts, saved media, playback, audio limitations | [REPLAY.md](REPLAY.md) |
| Implemented and verified platform scope | [HARDWARE-SUPPORT.md](HARDWARE-SUPPORT.md) |
| Runtime budgets and measurement procedure | [PERFORMANCE.md](PERFORMANCE.md) |
| Automated and manual verification | [TESTING.md](TESTING.md) |
| Open product/release work | [ROADMAP.md](ROADMAP.md) |
| Packaging and dependency obligations | [Packaging](packaging/README.md), [notices](packaging/THIRD-PARTY-NOTICES.md) |
| Native development entry points | [Tauri README](output/tauri-redunar/README.md) |
| Session graph calculations | [History calculations](output/tauri-redunar/HISTORY-CALCULATIONS.md) |
| Owner-controlled game validation | [Real-game testing](REAL-GAME-TESTING.md) |

Current guides were reconciled against the local source on September 16, 2026.
They describe implemented behavior and identify unresolved differences from
product intent. A documented limitation is not a claim that it is acceptable
for release. Dated test reports prove only the build and environment they name.

## Boundaries

Redunar does not require an account or collect telemetry.
Steam discovery and artwork use local files; artwork is copied into Redunar's
private cache. Saved games remain in Library after uninstall, with launch
disabled when installation checks determine they are missing.

Capture uses a private launch-time Vulkan layer and hardware video encoding on
supported configurations. Overlay visibility can change within a prepared game;
this is not late injection. Replay audio intentionally falls back to an output monitor when game-owned audio
cannot be identified; it can include other applications. See REPLAY.

The browser design reference and marketing website are maintained outside this
source distribution. Fictional design fixtures must not substitute for native
playback or game tests. This source tree builds without either website.

## License

Redunar source code is licensed under the GNU General Public License version 3
or later (`GPL-3.0-or-later`). See [LICENSE](LICENSE). Copyright is identified
under the public project name Redunar; individual authors retain copyright in
their contributions unless they assign it separately. Third-party components
retain their own licenses and notices. See
[third-party notices](packaging/THIRD-PARTY-NOTICES.md) and the generated
[locked dependency inventory](packaging/DEPENDENCY-LICENSES.md).

This repository contains the current Tauri application, shared services, tests,
packaging, and release documentation. The marketing website is maintained
separately.
