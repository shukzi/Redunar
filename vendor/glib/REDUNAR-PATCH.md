# GLib 0.18 compatibility security patch

Upstream: https://crates.io/crates/glib/0.18.5
Archive SHA-256: `233daaf6e83ae6a12a52055f568f9d7cf4671dabb78ff9560ab6da230ce00ee5`
License: MIT; upstream `LICENSE` and `COPYRIGHT` are retained unchanged.

Redunar retains this published source for the existing GTK3/WebKitGTK 4.1
stack. The only upstream source modification is the two-line mutable
out-argument repair in `src/variant_iter.rs`, backported from gtk-rs-core
commit `b5a4071e439bef2b5eea76c3aa25e5ae84839e34`:
https://github.com/gtk-rs/gtk-rs-core/pull/1343

This fixes RUSTSEC-2024-0429 without introducing a second GLib major or
changing the GTK3 ABI. The package keeps its upstream version; a version-only
advisory scan may still report 0.18.5. Verify the path override and patched
source, and run the optimized integration regression before acceptance:

```sh
cargo test --locked --offline --release --manifest-path output/tauri-redunar/src-tauri/Cargo.toml --test glib_variant_strings --target-dir .redunar-build/checks/tauri
```

The regression uses GTK's actual re-export of GLib and exercises forward,
reverse, nth, nth_back, last, mixed-end, empty and exhausted string iteration.
`tools/tests/test_dependency_security.py` verifies the complete vendored source
against recorded upstream file hashes and the effective Tauri override.

Remove this override only after the GTK3 dependency family supplies the fix
or the production dependency graph no longer contains affected GLib. No
claim is made that the rest of the GTK3 family is actively maintained.
