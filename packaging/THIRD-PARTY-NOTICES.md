# Third-party notices

Redunar application code, Vulkan layer code, shaders, protocol, tests, and
packaging metadata are maintained as original project work. Changing copied
code would not remove its original license obligations, so clean-room work is
the project rule.

The application bundles a small selected set of Lucide SVG icons. Their MIT
notice is retained verbatim at
`packaging/LICENSE-Lucide.txt` and must be included in every
binary distribution that ships those icons.

The in-game overlay embeds bounded antialiased subsets generated from Noto
Sans Mono Regular and Noto Sans Regular (Copyright 2013 Google LLC). The
subsets are modified font software under the SIL Open Font License 1.1.
The overlay subset asset does not include its source TTF; the Tauri UI separately
embeds unmodified TTFs as described below. Redunar does not use these names as
its product name. The required notice and license are retained verbatim at
`crates/redunar-core/src/LICENSE-Noto-Sans-Mono.txt` and must accompany every
binary distribution that ships either subset.

Rust, JavaScript, and system-library dependencies retain their own licenses.
`DEPENDENCY-LICENSES.md` is generated from both exact locked Cargo workspaces
for the supported x86_64 Linux target and from the Tauri npm lockfile. It
includes the dependency metadata and license files present in downloaded
packages. Regenerate and review it for each release; packages whose published
archive omits a standalone license file remain explicitly listed for manual
upstream verification.

The private Steam launch bridge uses the `nix` crate's safe `SO_PEERCRED`
wrapper. `nix` is MIT-licensed; its upstream copyright and license text must be
included by that generated release dependency inventory.

The Tauri workspace embeds unmodified Noto Sans and Noto Sans Mono TTFs under
SIL OFL 1.1; the included Noto notice also covers these original fonts.

The embedded web frontend uses `@tauri-apps/api` under its Apache-2.0 OR MIT
license choice. Both upstream license texts are retained under
`output/tauri-redunar/licenses/tauri-api` and accompany the binary packages.

The local artwork reader directly uses the already locked `libc` 0.2.189 crate
for Linux no-follow/nonblocking file-open flags. Its MIT / Apache-2.0 license
choice is compatible with GPL-3.0-or-later. Both upstream texts are retained in
`output/tauri-redunar/licenses/libc` and included in this local RPM. No upstream
source was copied or modified. This does not replace the full release inventory.

Optional Steam artwork is read from the user's existing local cache at runtime
and preserved in Redunar's private artwork store for later display, including
after uninstall. These remain user-local third-party game assets, not newly
licensed bundled artwork.
No game posters, banners, or fictional game images are included in the package;
missing, unsupported, or oversized artwork falls back to Redunar's neutral icon.

Media preparation/export invokes externally installed FFmpeg/FFprobe. Embedded
WebKit playback uses the distribution's GStreamer libav plugin. These are runtime
package requirements, not copied/bundled source or codec binaries. The reviewed
Fedora/RPM Fusion FFmpeg/freeworld providers declare GPL-3.0-or-later/GPLv3+;
the installed GStreamer libav provider declares LGPLv2+, compatible with this
GPL-3.0-or-later project. The providers retain their own packaged notices. Other
distributions' selected providers must be reviewed rather than inheriting this
specific package inventory as a universal license statement.
