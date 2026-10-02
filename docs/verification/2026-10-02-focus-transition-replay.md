# Focus-transition Replay investigation

October 2, 2026; CachyOS x86_64, niri, Rust 1.97.0. Based on e589fc31 plus
the local runtime-efficiency changes and the corrections described here.

## Observed evidence

The owner reported visual/audio stutter in both Redunar and an external player,
apparently during window switching. Two existing MKVs were inspected read-only:

- About 29.02 seconds with 383 video packets and 1,451 audio packets.
- About 29.23 seconds with 856 video packets and 1,461 audio packets.
- Video PTS were strictly increasing, but long sections had only one or two
  frames per second, interspersed with sections near 90 FPS.
- Audio packet spacing remained continuous at 20 ms. Decoded amplitude analysis
  found silence aligned with the sparse-video periods; in the first clip the
  non-silent interval was approximately 4.3–8.3 seconds, matching dense video.
- Native session history also contained low presentation-FPS observations.
- Production-equivalent video-copy/AAC preparation preserved all video PTS.
  Plain WebKit and the full production frontend/native playback probe played
  the 29.224-second prepared recording to completion without a stalled clock.
  This does not recreate pictures or sound absent from the source.

The evidence supports background game/compositor behavior rather than damage
introduced only by Redunar playback. It does not identify a specific niri or
game preference. No compositor/game settings were changed and no game was
launched by the agent. Original recordings were not modified or removed.

## Independent review and corrections

The owner-requested abbara/gpt-6.1-sol--fast model with ultra reasoning performed
an independent read-only review and a follow-up review of the patch. Both reviews
agreed with the diagnosis and the two independently identified timing defects:

1. Fixed Vulkan/OpenGL capture now reanchors the first frame after a missed
   interval to its actual presentation time; ordinary deadline cadence remains.
2. MP4 fragment endings now extend the preceding sample to the next fragment's
   first PTS. Rescaled clip-relative timestamps avoid cumulative rounding loss.

Neither defect explains the owner's Variable-mode MKVs by itself. These fixes
correct real timing edge cases without inventing gameplay frames or audio.

The UI now explains unfocused-game behavior and offers Retry activation for
configured but unavailable/inactive shortcuts. Retry preserves pending drafts;
helper EOF retains the useful activation error. This supports saving while the
game stays focused, after normal input access has been configured.

## Verification

- Full validation passed: 577 Rust tests, 12 ignored; 63 Node tests; 12 Python
  tooling tests; both workspace checks, formatting, strict Clippy, frontend build.
- Full report: .redunar-build/reports/20261002T125524Z-full.3g9p6i.log.
  Initial/final source manifests match. Manifest SHA-256:
  cc29b2344b619daa7d49c634deaf5bb0f0d80dfa75e971c733518ee9e1fbe053.
- Regressions cover fixed-rate pause recovery, fractional timing, MP4 keyframe
  and byte-limit boundaries, sub-microsecond rejection/temporary cleanup, and
  synthetic 90 FPS → 1 FPS → 90 FPS with continuous audio and a silent middle.
- All 77 workspace WebKit checks and the Replay WebKit fixture passed. The retry
  action was visually inspected. One concurrent Xvfb run exited during display
  setup; the sequential rerun passed without relaxing assertions.
- Sustained native playback of a private copy: first ready at 751 ms, eight
  filmstrip samples, maximum position 29.062 seconds, zero stalled clock samples.
- Native production build passed. Fresh executable SHA-256:
  91fdaaaf56fe310e40fab6228189aa5012f5dd99cf3098146f376744828cc090.
- The owner confirmed the game session had ended before restart. The old test
  window closed but its process remained alive; that exact verified process was
  stopped with SIGTERM. The prior payload was retained in the private test area.
- The fresh process and matching sidecars were verified, with primary ownership
  and a shortcut helper holding six input-device descriptors. It opened on the
  desktop-default native Wayland path rather than the test launcher's inherited
  X11 override. The saved test profile, shortcuts, and clips were retained.

The evidence file was added after validation; implementation files were not
changed afterward. A newly recorded foreground clip from the patched build is
still needed for live acceptance. Suggested control: keep the game foreground
for at least 30 seconds, save using its configured shortcut, then inspect the
clip. This is not an installed-package, cross-distribution, or FPS-gain claim.
