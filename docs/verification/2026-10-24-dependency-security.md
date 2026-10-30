# Existing Tauri dependency security repair

Scope: production Tauri app, based on `7bd76f9`, prepared October 24, 2026
for v0.1.29. Results and exact build manifests remain in the candidate’s
`.redunar-build/reports` and `.redunar-build/linux` directories.

The production native manifest requires Tauri 2.12.5; the lock resolves
Tokio 1.54.1. Tauri’s official advisory GHSA-w28w-mhc8-qvjv affects
2.0.0 through 2.11.5 and is fixed from 2.11.6. This is checked explicitly
because a package scan may omit it. The frontend resolves Vite 6.4.5,
source-map-js 1.2.2 and Tauri API 2.12.4.

GTK3/WebKitGTK requires GLib 0.18.5. The single effective GLib copy is the
path override under `vendor/glib`, with the upstream mutable out-argument
repair for RUSTSEC-2024-0429. Its provenance, original MIT notices, exact
source modification and optimized regression command are recorded in
[vendor/glib/REDUNAR-PATCH.md](../../vendor/glib/REDUNAR-PATCH.md). A
version-only scanner may still match the original version; do not describe
that match as an unpatched runtime or suppress it without checking the source.

The five `unic-*` unmaintained notices in the earlier audit are absent from
current merged main and this candidate. Current `tauri-utils` uses a newer
`urlpattern` dependency without those crates. `proc-macro-error` 1.0.4 remains
a build-time dependency of `glib-macros` and `gtk3-macros`. Its unmaintained
notice is retained as maintenance debt, not misreported as a runtime exploit.
Its direct parents have no compatible maintained release removing it.
Replacing it would require additional macro forks or a toolkit dependency
transition; no such change is included in this focused security repair.
Reassess the notice on the next dependency update.

There is no persisted-format migration, native capability expansion or
change to the service/UI permission boundary. Automated fixtures do not
qualify installed upgrades, real games, hardware capture, desktop timing,
NVIDIA or other-distribution runtime behavior.
