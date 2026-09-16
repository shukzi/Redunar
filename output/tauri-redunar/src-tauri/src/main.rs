#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod artwork;
mod backend;
mod catalog;
mod clip_export;
mod clip_metadata;
mod hotkeys;
mod installation;
mod launch_plan;
mod media;
mod media_tools;
mod playback;
mod profiles;
mod replay_menu_window;
mod runtime;
mod sessions;
mod tray;
mod updates;
use tauri::{Listener, Manager};

const APP_ID: &str = "com.redunar.Redunar";
#[cfg(target_os = "linux")]
const WEBKIT_DMABUF_ENV: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";

fn persistable_window_state(width: u32, height: u32, maximized: bool) -> Option<(i32, i32, bool)> {
    if width == 0 || height == 0 {
        return None;
    }
    Some((
        i32::try_from(width).ok()?,
        i32::try_from(height).ok()?,
        maximized,
    ))
}

fn persist_window_state(window: &tauri::Window) {
    if backend::other_owner() {
        return;
    }
    let Ok(size) = window.inner_size() else {
        return;
    };
    let Ok(maximized) = window.is_maximized() else {
        return;
    };
    let Some((width, height, maximized)) =
        persistable_window_state(size.width, size.height, maximized)
    else {
        return;
    };
    let _ = backend::service().set_window_state(width, height, maximized);
}

#[cfg(target_os = "linux")]
fn configure_webkit_renderer() {
    // WebKitGTK's DMA-BUF path can crash while its WebKitWebProcess is
    // tearing down the GBM/DRM device. Keep an explicit user override, but
    // prefer the software-backed renderer for Redunar's short-lived overlay
    // and settings webviews so closing the app does not produce a system
    // crash report.
    if std::env::var_os(WEBKIT_DMABUF_ENV).is_none() {
        std::env::set_var(WEBKIT_DMABUF_ENV, "1");
    }
}

#[cfg(test)]
mod tests {
    use super::persistable_window_state;

    #[test]
    fn window_state_persists_maximized_flag() {
        assert_eq!(
            persistable_window_state(1_280, 720, true),
            Some((1_280, 720, true))
        );
        assert_eq!(
            persistable_window_state(1_280, 720, false),
            Some((1_280, 720, false))
        );
    }

    #[test]
    fn zero_sized_window_is_not_persisted() {
        assert_eq!(persistable_window_state(0, 720, false), None);
        assert_eq!(persistable_window_state(1_280, 0, true), None);
    }
}

fn main() {
    #[cfg(target_os = "linux")]
    configure_webkit_renderer();
    // GTK 3 otherwise advertises the executable name ("redunar-tauri") as
    // the xdg_toplevel app_id on Wayland. KDE cannot match that surface to
    // com.redunar.Redunar.desktop and falls back to its generic Wayland
    // icon. This must run before Tauri initializes GTK or creates a window.
    #[cfg(target_os = "linux")]
    gtk::glib::set_prgname(Some(APP_ID));
    backend::other_owner();
    // Load the saved replay configuration once, outside the polling path.
    let _ = backend::service().replay_runtime_status();
    let monitor = backend::service().start_monitor();
    let sessions = sessions::Sessions::new(backend::service(), monitor.reader())
        .expect("Could not start the session supervisor");
    let app = tauri::Builder::default()
        .setup(|app| {
            tray::setup(app)?;
            app.state::<hotkeys::ShortcutMonitor>()
                .set_app_handle(app.handle().clone());
            let handle = app.handle().clone();
            app.listen("replay-menu-open-settings", move |_| {
                if let Some(window) = handle.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                    let _ = window.eval("location.hash = '#global'");
                }
            });
            if let Some(window) = app.get_webview_window("main") {
                if let Ok(saved) = backend::service().app_preferences() {
                    if saved.window_maximized {
                        let _ = window.maximize();
                    } else {
                        let _ = window.set_size(tauri::Size::Physical(tauri::PhysicalSize::new(
                            saved.window_width as u32,
                            saved.window_height as u32,
                        )));
                    }
                }
            }
            // Shortcut monitoring is native and app-lifetime scoped. If the
            // user's evdev permissions are unavailable, the rest of the app
            // remains usable and the UI reports the monitor as unavailable.
            if let Err(error) = app.state::<hotkeys::ShortcutMonitor>().activate() {
                eprintln!("Redunar shortcuts unavailable: {error}");
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "replay-menu" {
                if let tauri::WindowEvent::Focused(false) = event {
                    let should_hide = backend::service()
                        .replay_preferences()
                        .map(|preferences| preferences.close_overlay_on_outside_click)
                        .unwrap_or(false);
                    if should_hide {
                        backend::service()
                            .game_session_coordinator()
                            .close_replay_menu();
                        let _ = window.hide();
                    }
                }
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    backend::service()
                        .game_session_coordinator()
                        .close_replay_menu();
                    let _ = window.hide();
                }
                return;
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Do not write preferences from every native move/resize
                // notification. GTK can emit those events repeatedly while
                // the user drags a border, and synchronous preference I/O in
                // that path makes the compositor interaction feel sticky.
                // Capture the final usable geometry once when the window is
                // actually closing instead.
                persist_window_state(window);
                // Only intercept close when the user explicitly enabled
                // close-to-tray. Otherwise let Tauri perform its normal close
                // and ExitRequested/Exit lifecycle so WebKit can shut down
                // in order.
                if tray::close_to_tray(window.app_handle()) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .manage(clip_export::ClipExports::default())
        .manage(hotkeys::ShortcutMonitor::default())
        .manage(playback::Playback::default())
        .manage(backend::Monitor(std::sync::Mutex::new(monitor)))
        .manage(sessions)
        .invoke_handler(tauri::generate_handler![
            backend::daemon_status,
            backend::diagnostics_snapshot,
            runtime::app_preferences,
            runtime::set_close_to_tray,
            runtime::set_automatic_updates,
            runtime::check_for_updates,
            runtime::install_update,
            runtime::replay_preferences,
            runtime::replay_display_capability,
            runtime::set_replay_save_parent,
            runtime::set_replay_overlay_behavior,
            runtime::set_replay_initial_save_duration,
            runtime::hide_replay_menu,
            clip_metadata::clip_metadata,
            catalog::catalog_games,
            installation::game_installation_statuses,
            artwork::game_poster,
            artwork::game_banner,
            catalog::pick_executable,
            catalog::add_game,
            catalog::discover_games,
            catalog::import_discovered_games,
            catalog::update_game_launch,
            catalog::remove_game,
            catalog::steam_setup_status,
            catalog::save_game_profile,
            media::replay_clips,
            media::clip_playback_path,
            media::clip_thumbnail,
            media::open_replay_folder,
            media::open_clip_external,
            clip_export::clip_export_status,
            clip_export::export_replay_clip,
            clip_export::cancel_clip_export,
            hotkeys::shortcut_status,
            hotkeys::activate_shortcuts,
            hotkeys::deactivate_shortcuts,
            profiles::global_settings,
            profiles::save_global_settings,
            profiles::save_shortcuts,
            runtime::session_history,
            runtime::replay_storage_status,
            runtime::module_statuses,
            runtime::session_status,
            runtime::end_session,
            sessions::launch_game,
            runtime::replay_runtime_status,
            runtime::save_replay,
            runtime::delete_replay_clip,
        ])
        .build(tauri::generate_context!())
        .expect("Could not open Redunar");
    app.run(|app, event| {
        if let tauri::RunEvent::ExitRequested { ref api, .. } = event {
            // Keep the app visible if session cleanup needs user attention.
            if let Err(error) = app.state::<sessions::Sessions>().end() {
                api.prevent_exit();
                tray::show_window(app);
                if let Some(window) = app.get_webview_window("main") {
                    let message = serde_json::to_string(&format!(
                        "Session cleanup requires attention: {error}"
                    ))
                    .unwrap_or_default();
                    let _ = window.eval(format!(
                        "window.dispatchEvent(new CustomEvent('native-error',{{detail:{message}}}))"
                    ));
                }
            }
        }
        if let tauri::RunEvent::Exit = event {
            app.state::<sessions::Sessions>().shutdown();
            app.state::<clip_export::ClipExports>().shutdown();
            app.state::<hotkeys::ShortcutMonitor>().shutdown();
            app.state::<playback::Playback>().shutdown();
            backend::shutdown();
            if let Ok(mut monitor) = app.state::<backend::Monitor>().0.lock() {
                monitor.shutdown();
            }
        }
    });
}

#[cfg(test)]
mod identity_tests {
    use super::APP_ID;

    #[test]
    fn runtime_identity_matches_the_tauri_identifier() {
        let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
        assert_eq!(context.config().identifier, APP_ID);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn webkit_renderer_workaround_uses_the_supported_environment_key() {
        assert_eq!(super::WEBKIT_DMABUF_ENV, "WEBKIT_DISABLE_DMABUF_RENDERER");
    }
}
