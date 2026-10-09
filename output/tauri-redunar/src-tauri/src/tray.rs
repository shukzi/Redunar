use crate::tray_native::{Action, Icon, NativeTray};
use std::sync::{Arc, Mutex};
use tauri::Manager;

#[derive(Default)]
struct TrayState(Arc<Mutex<Option<NativeTray>>>);

pub fn show_window(app: &tauri::AppHandle) {
    crate::background_start::retain_window(app);
    if let Some(window) = app.get_webview_window("main") {
        let shown = window.show().is_ok();
        let _ = window.unminimize();
        let _ = window.set_focus();
        if shown {
            remove_if_disabled(app);
        }
    }
}

pub(crate) fn remove_if_disabled(app: &tauri::AppHandle) {
    // Once the automatic owner has a reachable window, the saved preference
    // owns icon visibility again. Mapping can still be queued here; setup's map
    // observer completes removal once reachable. Never invoke hidden-window
    // recovery for this transition, because that would count the fallback as Open.
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if crate::backend::service()
            .app_preferences()
            .is_ok_and(|preferences| !preferences.close_to_tray)
            && !handle
                .try_state::<crate::background_start::State>()
                .is_some_and(|state| state.close_pending())
            && handle
                .get_webview_window("main")
                .is_some_and(|window| window.is_visible().unwrap_or(false))
        {
            let _ = set_enabled(&handle, false);
        }
    });
}

pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    app.manage(TrayState::default());
    #[cfg(target_os = "linux")]
    if let Some(window) = app.get_webview_window("main") {
        if let Ok(native) = window.gtk_window() {
            use gtk::prelude::*;
            let tray = Arc::downgrade(&app.state::<TrayState>().0);
            let owner = app
                .try_state::<crate::background_start::State>()
                .map(|state| state.inner().clone());
            native.connect_map_event(move |_, _| {
                if crate::backend::service()
                    .app_preferences()
                    .is_ok_and(|prefs| !prefs.close_to_tray)
                    && !owner.as_ref().is_some_and(|state| state.close_pending())
                {
                    if let Some(state) = tray.upgrade() {
                        // set_enabled may already own this lock while recovering
                        // a hidden window; that caller removes its own icon.
                        if let Ok(mut tray) = state.try_lock() {
                            tray.take();
                        }
                    }
                }
                gtk::glib::Propagation::Proceed
            });
        }
    }
    if crate::backend::service()
        .app_preferences()
        .is_ok_and(|preferences| preferences.close_to_tray)
    {
        if let Err(error) = set_enabled(app.handle(), true) {
            eprintln!("Redunar tray unavailable: {error}");
            if !app
                .try_state::<crate::background_start::State>()
                .is_some_and(|state| state.may_auto_exit())
            {
                show_window(app.handle());
            }
        }
    }
    Ok(())
}

/// Native ownership controls both registration and the close behavior.
pub fn set_enabled(app: &tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let state = app.state::<TrayState>();
    let mut tray = state
        .0
        .lock()
        .map_err(|_| "The tray state is unavailable")?;
    if !enabled {
        // Recover the user's way back before removing the connection/icon.
        // Retain the icon and preference if native window recovery fails.
        if let Some(window) = app.get_webview_window("main") {
            if !window.is_visible().map_err(|error| error.to_string())? {
                window.show().map_err(|error| error.to_string())?;
                crate::background_start::retain_window(app);
                window.set_focus().map_err(|error| error.to_string())?;
            }
        }
        tray.take();
        return Ok(());
    }
    if tray.as_ref().is_some_and(NativeTray::is_running) {
        return Ok(());
    }
    tray.take();
    let image = app
        .default_window_icon()
        .ok_or("The tray icon is unavailable")?;
    let icon = Icon::from_rgba(image.width(), image.height(), image.rgba())?;
    let app = app.clone();
    let action = Arc::new(move |action| {
        let handle = app.clone();
        // Never wait on GTK from the bus worker: disabling/shutdown joins it.
        let _ = app.run_on_main_thread(move || match action {
            Action::Open | Action::Recover => show_window(&handle),
            Action::Quit => handle.exit(0),
        });
    });
    *tray = Some(NativeTray::start(icon, action)?);
    Ok(())
}

pub fn close_to_tray(app: &tauri::AppHandle) -> bool {
    let enabled = crate::backend::service()
        .app_preferences()
        .is_ok_and(|preferences| preferences.close_to_tray);
    should_hide(enabled, enabled && has_item(app) && registered(app))
}

fn should_hide(enabled: bool, registered: bool) -> bool {
    enabled && registered
}

pub(crate) fn registered(app: &tauri::AppHandle) -> bool {
    app.try_state::<TrayState>().is_some_and(|state| {
        state
            .0
            .lock()
            .is_ok_and(|tray| tray.as_ref().is_some_and(NativeTray::registered))
    })
}

pub(crate) fn shutdown(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<TrayState>() {
        if let Ok(mut tray) = state.0.lock() {
            tray.take();
        }
    }
}

pub(crate) fn has_item(app: &tauri::AppHandle) -> bool {
    app.try_state::<TrayState>().is_some_and(|state| {
        state
            .0
            .lock()
            .is_ok_and(|tray| tray.as_ref().is_some_and(NativeTray::is_running))
    })
}

pub(crate) fn set_tooltip(app: &tauri::AppHandle, text: &str) {
    if let Some(state) = app.try_state::<TrayState>() {
        if let Ok(tray) = state.0.lock() {
            if let Some(tray) = tray.as_ref() {
                tray.set_tooltip(text);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_icon_is_square_rgba_with_one_byte_per_channel() {
        let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
        let icon = context.default_window_icon().expect("app icon");
        assert_eq!((icon.width(), icon.height()), (256, 256));
        assert_eq!(icon.rgba().len(), 256 * 256 * 4);
        assert!(Icon::from_rgba(icon.width(), icon.height(), icon.rgba()).is_ok());
    }

    #[test]
    fn close_keeps_a_reachable_window_without_preference_and_registration() {
        assert!(should_hide(true, true));
        assert!(!should_hide(false, true));
        assert!(!should_hide(true, false));
        assert!(!should_hide(false, false));
    }
}
