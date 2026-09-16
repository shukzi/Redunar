Native tray dependency notices
==============================

The Tauri tray-icon feature enables tray-icon 0.24.2 and libappindicator 0.9.0 (MIT OR Apache-2.0). GTK 0.18.2 (MIT) and libloading 0.7.4 (ISC), already present in the native dependency graph, are now used directly to check desktop registration and optional runtime availability. Sources come from the existing Cargo cache; no third-party implementation was copied into Redunar. These permissive licenses are compatible with Redunar. Retain these notices when distributing the native build.

Linux uses the system AppIndicator provider. Future packaging must include its own distribution obligations and runtime dependency declaration.
