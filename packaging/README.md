# Packaging contract

Current private Tauri package, reviewed September 14, 2026. The active package
is `redunar-app`, with version/release owned by
[`redunar-app.spec`](redunar-app.spec). No public release is implied.

## Build and verify

From the repository root:

```sh
tools/build-linux-release.sh
tools/stage-tauri-package.sh /absolute/empty/staging/root
tools/build-local-tauri-rpm.sh target/packages
tools/build-tauri-deb.sh target/packages
tools/build-tauri-arch-package.sh target/packages
tools/build-tauri-opensuse-rpm.sh target/packages
tools/build-tauri-portable.sh target/packages
REDUNAR_RELEASE_SIGNING_KEY=/absolute/private.pem tools/build-install-assets.sh target/install-assets
tools/check-tauri-release.sh
```

Staging and building do not install or run the app. Local development uses one
stable `target/packages/redunar-app.rpm`, overwritten by subsequent builds.
The full gate stages into temporary directories and checks metadata, package
contents, installer detection, scripts, helper permissions, and the native
privilege boundary.

Installation is a separate owner-controlled action. After upgrading, fully quit
any running Redunar/tray process and run
`tools/check-installed-tauri-runtime.sh` before acceptance. That read-only
check compares the installed binary and sidecars to the build; a new file on
disk cannot update an already-running process.

## Active layout and privilege boundary

- `/usr/bin/redunar-tauri`, with its embedded frontend.
- `/usr/bin/redunar-steam-launch` and `libredunar_capture_vulkan.so` beside the
  app, resolved into a private per-launch Vulkan/Steam configuration.
- `/usr/libexec/redunar-hotkey-helper`, running as the logged-in user.
- `/usr/lib/udev/rules.d/70-redunar-hotkeys.rules`, giving the active graphical
  session keyboard-event read access through logind `uaccess`.
- `com.redunar.Redunar.desktop`, matching AppStream metadata, canonical
  Comet R icons, and required notices/licenses.

The production identity follows the owned `redunar.com` namespace. The optional
local desktop-registration helper removes only a launcher carrying its own
`X-Redunar-Local-Tauri=true` marker. It never removes an unmanaged user
launcher. Redunar's application state and recordings remain under the shared
`redunar` state paths.

The only deliberate package post-install action reloads udev and retriggers
existing keyboard event devices so access can apply without reconnecting them.
No Polkit policy, setuid/setgid helper, file capability, global Vulkan layer,
Steam edit, monitoring startup, hardware-policy change, or network request belongs
in this package. The runtime never prompts for privilege elevation.

Steam Launch Options are derived from the verified absolute wrapper path. Keep
that path representable safely by the existing bridge format. Wrapper presence
is not proof the game is configured; verify its saved local options read-only.

## Runtime dependencies

The current spec explicitly requires GTK3, WebKitGTK 4.1, libdrm, PipeWire tools,
Opus, the `/usr/bin/ffmpeg` and `/usr/bin/ffprobe` files, and the architecture-specific
`libavcodec-freeworld` capability, and the GStreamer `libgstlibav.so` plugin used
by WebKit, in addition to discovered binary dependencies. A tray provider is optional; missing registration must leave normal window closing usable.

Replay capture uses the hardware Vulkan Video path. An FFmpeg encoder name is
not evidence that live hardware recording is supported. Audio uses system
`pw-cat` and `libopus.so.0`; its approved output-fallback behavior is documented in
[REPLAY.md](../REPLAY.md).

Playback preparation and trimmed export invoke FFmpeg separately from recording.
The executable file dependencies accept either `ffmpeg` or `ffmpeg-free` packages.
RPM Fusion's `ffmpeg-libs` also provides `libavcodec-freeworld(x86-64)`; that same
capability is provided by its standalone `libavcodec-freeworld` package used with
Fedora's free tools. Requiring the capability, rather than a specific FFmpeg
package, preserves a compatible installed stack while allowing the package
manager to install missing requirements. Redunar declares no multimedia package
Conflicts/Obsoletes and runs no package-swap command.

The complete codec capability covers the current H.264 decoding/libx264 export
and AAC needs; the app still checks actual encoder availability and reports
clip-specific missing support. WebKit uses the GStreamer libav plugin with those system codecs for embedded
playback; command-line FFmpeg alone does not install this plugin. Conversely,
codec plugins for another framework are not automatically usable by FFmpeg. Do not claim a limited provider is fully
compatible merely because an executable exists. A repository offering a required
provider must already be available; the app never adds repositories or installs
packages at runtime. Provider/version synchronization remains the distribution's
package-manager responsibility.

## One-command installer and release assets

The source-root [`install.sh`](../install.sh) is a publication template. Render
it for public distribution with `shukzi/Redunar` and the release verification
key:

```sh
tools/render-public-installer.sh shukzi/Redunar /absolute/output/install.sh packaging/release-signing-public.pem
```

The source template can also use `REDUNAR_GITHUB_REPOSITORY` for local plan
checks.

The installer accepts no prompts of its own. It may still require the normal
sudo or doas authentication required for a system installation. It downloads
one asset, `SHA256SUMS`, and `SHA256SUMS.sig` over HTTPS. OpenSSL verifies the
manifest with the public key embedded in the rendered installer before the
selected package checksum is trusted.

| Family | Release asset | Installation path |
| --- | --- | --- |
| Fedora/RHEL-like | `redunar-app-linux-x86_64.rpm` | `dnf` or `yum`, including dependency resolution |
| Debian/Ubuntu-like | `redunar-app-linux-amd64.deb` | noninteractive `apt-get`, including dependency resolution |
| Arch-like | `redunar-app-linux-x86_64.pkg.tar.zst` | dependency installation followed by native `pacman -U` ownership |
| openSUSE-like | `redunar-app-linux-opensuse-x86_64.rpm` | noninteractive `zypper`, including dependency resolution |

Current assets require x86_64 and glibc 2.36 or newer. The release binaries are
built with networking disabled in the pinned Debian 12 build image after its
one-time construction; their current highest referenced glibc symbol is 2.34.
Debian 12 is the support floor because it also supplies WebKitGTK 4.1. Alpine/musl, ARM,
NixOS, SteamOS, Bazzite, and other immutable/OSTree installations are rejected
before download because the current payload and host integration do not support
them safely. Distribution detection and archive construction are automated;
only Fedora 44 has installed-runtime evidence. Other families still require VM
installation, upgrade, uninstall, WebKit, codec, udev, Steam, and capture
validation before release support can be claimed.

The Fedora package requires a compatible complete codec provider already
available to DNF. The installer never enables RPM Fusion, Packman, or another
third-party repository. Arch and openSUSE now receive native package-manager
ownership for upgrade and removal, while the portable archive remains a manual
fallback artifact. Native package construction does not replace the outstanding
installation/runtime validation on those systems.

`tools/build-install-assets.sh` builds five stable asset names, adjacent checksum
files, `SHA256SUMS`, and its RSA/SHA-256 signature. It requires a private-key
path and refuses a key that does not match
[`release-signing-public.pem`](release-signing-public.pem). The private key stays
outside the source tree. The installer authenticates the manifest before using
its checksum, and corrupted assets or signatures fail before package installation.

The active RPM, DEB, Arch, and openSUSE package builders package the same staged
glibc-baseline payload. The portable archive exercises the common staged layout
but is not selected by the supported-family installer. A raw binary alone does
not supply WebKit, codecs, shortcut access, or desktop integration.

The app's capability checks stay independent of RPM or distribution names. Each
future platform needs appropriate dependency metadata and validation. Preserve
compatible installed providers, do not silently swap codecs or add repositories,
and verify the download before execution. Public hosting, GitHub automation,
and real cross-distribution acceptance remain separate release work.
Keep [third-party notices](THIRD-PARTY-NOTICES.md) and an exact locked dependency
inventory with distribution artifacts; preserve legal notices verbatim.
Regenerate [the dependency inventory](DEPENDENCY-LICENSES.md) with
`python3 tools/generate-license-inventory.py` after either Cargo lockfile or the
Tauri npm lockfile changes. Release checks reject a stale inventory and run the
shared `cargo-deny` license policy against both Rust workspaces.
