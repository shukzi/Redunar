# Producer handoff and compatibility qualification

Verified October 2, 2026 on CachyOS x86_64, with host Rust 1.97.0 and
Node 26.10.0. Branch `fix/nvidia-beta-readiness`, based on
`e589fc31abebb00396b152922c162a235db73ca9`. Existing runtime-efficiency,
focus-transition, NVIDIA beta, and logging changes were preserved.

This follows the [NVIDIA readiness report](2026-10-02-nvidia-readiness.md)
and resolves its pending compatibility-build and producer-handoff work.
It does not establish NVIDIA hardware or installed-runtime acceptance.

## Exact tested source

`tools/validate.sh release` passed with report
`.redunar-build/reports/20261002T203410Z-release.qEYBHB.log`.
The initial validation, final validation, and compatibility-build source
manifests match exactly. Their `source_sha256` is
`c7e24dfe9e1ddb566e7e392a8b626790300428c4f07853ae79f9ab2af5ee883b`.
The validation `.source.json` file itself has SHA-256
`f707990f939da502a8ea1dca44f50037a3c36a390441362eb186ffefceaeb0f2`.
The manifest identifies the uncommitted working source; the base commit alone
does not identify this build.

This report, its TESTING link and coverage description, and the historical
report's follow-up link were added after validation. Implementation and test
files remained unchanged; documentation links and consistency were checked
separately afterward.

## Changed behavior and ownership

- Export ordering is checked per selected or provisional producer. Accepted
  exports receive session-unique internal tokens, with the original reply
  endpoint and wire sequence retained until safe completion. A game can take
  over from a helper with lower or overlapping numbers without releasing the
  helper's unfinished input. Duplicate in-flight exports cannot cause an ACK.
- Producer confirmation counts only increasing exports. Readiness follows the
  confirmed source, including a smaller game surface. Foreign-session messages
  cannot change selection or ownership. Copied-frame telemetry resets its local
  watermark when the confirmed owner changes, including a rejected confirmation
  frame followed by a valid export.
- Queued, unsubmitted frames use safe release handling shared by both import
  callers. Failed acknowledgements retain their origin for worker retry;
  another producer's equal wire sequence cannot overwrite that route.
- All outstanding reply routes share a 64-entry cap, including queued and
  GPU-owned inputs and failed ACKs. The receiver pauses before receiving
  another descriptor at capacity, allowing bounded kernel/producer queues to
  supply backpressure. ACK sends and the shutdown wake are nonblocking.
- Accepted device recreation resets producer confirmation without discarding
  outstanding routes. Vulkan's export counter survives device destruction.
  Vulkan and OpenGL each bind a fresh private endpoint per startup, without
  unlinking another endpoint. Delayed ACKs cannot reach a replacement library
  incarnation or a process reusing the same PID.
- Session preparation reserves the entire 23-byte reply filename within
  Linux's 107-byte pathname socket limit before creating runtime files. The
  shared producer helper checks the same byte limit. Overlong paths return a
  clear startup error instead of silently failing in the game's capture layer.

Ownership lives in `capture_session/replay_release.rs`; shared endpoint rules
live in `redunar-capture/src/reply_endpoint.rs`. Contracts are documented in
[ARCHITECTURE](../../ARCHITECTURE.md), [REPLAY](../../REPLAY.md), and
[PERFORMANCE](../../PERFORMANCE.md).
No capture wire-format, saved profile/history/clip format, dependency, or
version change was introduced. User recordings and unrelated local work were
preserved.

## Verification and independent review

| Check | Result on October 2, 2026 |
| --- | --- |
| `tools/validate.sh release` | Passed: 666 Rust tests, 12 ignored; 64 Node tests; 13 Python tooling tests; both Cargo checks, strict Clippy, formatting, production frontend build, compatibility build, and package gate |
| Shared service | 268 passed, 6 hardware tests ignored; includes 13 new handoff, ownership, import-retry, capacity, shutdown, and path-boundary regressions |
| Vulkan capture | 93 passed, 2 hardware tests ignored; includes device destruction with an old outstanding copy proof and delayed release |
| OpenGL capture | 33 passed; startup tests verify distinct endpoints for repeated and different PIDs |
| Shared capture protocol | 16 passed; includes multibyte path-length boundaries for both APIs |
| Real Unix transport fixtures | Passed with private sockets and dummy descriptors; kernel-queued SCM_RIGHTS exports resume after route-cap recovery, and saturated shutdown returns |
| Compatibility payloads | All six binaries require at most glibc 2.34, below the 2.36 floor; recorded digests match the current files and capture-component manifest |
| Packaging | DEB, portable archive, native Arch package, openSUSE RPM, and local Fedora RPM built and passed the release gate's structure, payload, dependency, and metadata checks |
| Installer/signing fixtures | Private installer tests and generated temporary-key signature verification passed; no production signing key or installation used |
| Licensing and assets | Both locked license checks, generated license inventory, desktop metadata, and offline AppStream checks passed |
| Documentation | Local links, command references, whitespace, and post-validation source differences checked |

The release gate repeats the Tauri and Node tests; the totals above count the
primary suites once. Hardware-dependent ignored tests were not enabled.
Tests use private fixture state and fake GPU callbacks, not a game or NVIDIA
driver session. Existing test fixture prefixes were shortened to fit the full
reply-path contract, including the release gate's `/dev/shm` rerun.

An independent read-only reviewer examined the production receiver, imports,
release routing, native producer lifetime, capacity handling, and cleanup.
Review corrections made the shutdown wake nonblocking, preserved sequence
identity across device recreation, added fresh incarnation endpoints, and
reserved the full socket pathname. The final review found no outstanding P1
or P2 findings in this delta. The release gate passed after all corrections.

RPM queries on this CachyOS host printed missing `/var/lib/rpm/.rpm.lock`
messages. Queries returned success and all package assertions passed; the host
RPM database was not initialized or modified to suppress those messages.

## Compatibility environment and artifacts

The pinned Debian 12 base is cached locally:
`docker.io/library/rust@sha256:408fe88047cef61a2087653b0c5255fa51c0f2d6d94ddedd7a2562a9b91a46f6`.
The prepared image is `localhost/redunar-build-glibc-2.36:bookworm`, image ID
`sha256:e22e984042ee73a68b8a7941b3a744f774ff7b7229c3b143ce838a362a8d3f9b`.
The pinned Arch packaging image is also cached. Compatibility compilation and
package-container runs used the existing offline, network-disabled commands.

Current compatibility artifacts are in
`.redunar-build/linux/native/tauri/release/`. The baseline and complete runtime
digest records are `.redunar-build/linux/linux-release-baseline.txt` and
`.redunar-build/linux/runtime.sha256`.

| Artifact | SHA-256 |
| --- | --- |
| `redunar-tauri` | `e928e3b448863c15326a43e0fbe4dd3b2a292faf93d74ea3a2944655d1f63684` |
| `redunar-steam-launch` | `0dd19f8426aeb0e10e5b4cf73c8ea98cf14f8f228a1d7f53c469d69d2b168f4b` |
| `redunar-hotkey-helper` | `ea74dce74d3bdd9783d44b537c0f95a0f6569e19985406fdcaeb269068411209` |
| `redunar-update-helper` | `8d895839bacca29582344839148e5655982a9a8a506ffe4b1e2e21ab33346439` |
| `libredunar_capture_vulkan.so` | `40cc8236405c94d08bc4d7dee5b1045cec5d882ec0f08197f895863994c7c027` |
| `libredunar_capture_opengl.so` | `57db9cc55bfe92ccbd95a5cdc07d49f9a720b6651dacee5b8da275ca0ea24539` |

## Remaining acceptance scope

The automated compatibility and handoff work is complete. No package was
installed, published, or signed with a production key. The gate cleans its
temporary packages; this dirty branch is not a retained immutable release
candidate. Follow [RELEASING](../RELEASING.md) on the integrated, committed
candidate before publication, and verify the selected installed payloads
before a separately authorized hardware run.

NVIDIA recording, audio synchronization, real-game handoff, other-distribution
runtime behavior, and performance still require human hardware evidence.
The existing bounded presentation-retirement and stuck-driver-call limitations
remain. Provisional copied-frame diagnostics can still aggregate before an
owner is confirmed; they do not grant or release production DMA-BUF ownership.
The [first NVIDIA hardware test](../../TESTING.md#nvidia-beta-first-hardware-test)
uses the official package's separate Beta access and Debug log switches.
