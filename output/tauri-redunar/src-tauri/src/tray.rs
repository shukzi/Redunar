use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager,
};

const TRAY_ID: &str = "redunar";

pub fn show_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    if crate::backend::service()
        .app_preferences()
        .is_ok_and(|preferences| preferences.close_to_tray)
    {
        if let Err(error) = set_enabled(app.handle(), true) {
            eprintln!("Redunar tray unavailable: {error}");
        }
    }
    #[cfg(target_os = "linux")]
    {
        let handle = app.handle().clone();
        gtk::gio::bus_watch_name(
            gtk::gio::BusType::Session,
            "org.kde.StatusNotifierWatcher",
            gtk::gio::BusNameWatcherFlags::NONE,
            |_, _, _| {},
            move |_, _| {
                if crate::backend::service()
                    .app_preferences()
                    .is_ok_and(|preferences| preferences.close_to_tray)
                    && handle.tray_by_id(TRAY_ID).is_some()
                {
                    show_window(&handle);
                }
            },
        );
    }
    Ok(())
}

/// Update native icon visibility as well as the close behavior.
pub fn set_enabled(app: &tauri::AppHandle, enabled: bool) -> Result<(), String> {
    if !enabled {
        // Removing the user's only way back must first recover a hidden window.
        if let Some(window) = app.get_webview_window("main") {
            if !window.is_visible().map_err(|error| error.to_string())? {
                window.show().map_err(|error| error.to_string())?;
                window.set_focus().map_err(|error| error.to_string())?;
            }
        }
        if let Some(icon) = app.tray_by_id(TRAY_ID) {
            icon.set_visible(false).map_err(|error| error.to_string())?;
        }
        return Ok(());
    }
    if let Some(icon) = app.tray_by_id(TRAY_ID) {
        // Linux AppIndicator retains its exported object after a tray handle is
        // dropped. Reuse one registration to avoid duplicate D-Bus paths and
        // menu handlers on repeated toggles. Disabled startup creates nothing.
        return icon.set_visible(true).map_err(|error| error.to_string());
    }
    // Tauri's optional Linux provider must be checked before its loader runs.
    #[cfg(target_os = "linux")]
    let _indicator_library = indicator_library().ok_or("No AppIndicator provider is installed")?;
    let open = MenuItem::with_id(app, "redunar-open", "Open Redunar", true, None::<&str>)
        .map_err(|error| error.to_string())?;
    let quit = MenuItem::with_id(app, "redunar-quit", "Quit Redunar", true, None::<&str>)
        .map_err(|error| error.to_string())?;
    let menu = Menu::with_items(app, &[&open, &quit]).map_err(|error| error.to_string())?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or("The tray icon is unavailable")?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("Redunar")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "redunar-open" => show_window(app),
            "redunar-quit" => app.exit(0),
            _ => {}
        })
        .build(app)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "linux")]
fn indicator_library() -> Option<libloading::Library> {
    ["libayatana-appindicator3.so.1", "libappindicator3.so.1"]
        .iter()
        .find_map(|name| {
            // SAFETY: fixed system library names, never user-supplied paths; no
            // symbols are invoked here. Tauri retains its own handle after setup.
            unsafe { libloading::Library::new(name).ok() }
        })
}

pub fn close_to_tray(app: &tauri::AppHandle) -> bool {
    let enabled = crate::backend::service()
        .app_preferences()
        .is_ok_and(|preferences| preferences.close_to_tray);
    should_hide(enabled, app.tray_by_id(TRAY_ID).is_some() && registered())
}

fn should_hide(enabled: bool, registered: bool) -> bool {
    enabled && registered
}

#[cfg(target_os = "linux")]
fn registered() -> bool {
    use gtk::glib::variant::ToVariant;
    let Ok(bus) = gtk::gio::bus_get_sync(gtk::gio::BusType::Session, gtk::gio::Cancellable::NONE)
    else {
        return false;
    };
    // Check this process's registration, not merely the presence of a tray host.
    // This bounded query only runs on close, never in the measurement loop.
    bus.call_sync(
        Some("org.kde.StatusNotifierWatcher"),
        "/StatusNotifierWatcher",
        "org.freedesktop.DBus.Properties",
        "Get",
        Some(
            &(
                "org.kde.StatusNotifierWatcher",
                "RegisteredStatusNotifierItems",
            )
                .to_variant(),
        ),
        None,
        gtk::gio::DBusCallFlags::NONE,
        250,
        gtk::gio::Cancellable::NONE,
    )
    .ok()
    .and_then(|reply| reply.get::<(gtk::glib::Variant,)>())
    .and_then(|(items,)| items.get::<Vec<String>>())
    .is_some_and(|items| {
        owns_registration(&items, std::process::id(), bus.unique_name().as_deref())
    })
}

#[cfg(target_os = "linux")]
fn owns_registration(items: &[String], pid: u32, bus_name: Option<&str>) -> bool {
    let prefix = format!("org.kde.StatusNotifierItem-{pid}-");
    items.iter().any(|item| {
        item.starts_with(&prefix)
            || bus_name.is_some_and(|name| {
                item.strip_prefix(name)
                    .is_some_and(|path| path.starts_with('/'))
            })
    })
}

#[cfg(not(target_os = "linux"))]
fn registered() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_icon_is_square_rgba_with_one_byte_per_channel() {
        // Tauri accepts a 16-bit PNG at build time but its tray writer expects
        // exactly four bytes per pixel. Validate the actual decoded asset.
        let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
        let icon = context.default_window_icon().expect("app icon");
        assert_eq!((icon.width(), icon.height()), (256, 256));
        assert_eq!(icon.rgba().len(), 256 * 256 * 4);
    }

    #[test]
    fn close_keeps_a_reachable_window_without_preference_and_registration() {
        assert!(should_hide(true, true));
        assert!(!should_hide(false, true));
        assert!(!should_hide(true, false));
        assert!(!should_hide(false, false));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn another_apps_tray_does_not_allow_hiding_this_window() {
        let entries = vec!["org.kde.StatusNotifierItem-123-1/StatusNotifierItem".into()];
        assert!(owns_registration(&entries, 123, None));
        assert!(!owns_registration(&entries, 12, None));
        assert!(!owns_registration(&entries, 1234, None));
        assert!(!owns_registration(&[], 123, None));
        let unique = vec![":1.123/org/ayatana/NotificationItem/redunar".into()];
        assert!(owns_registration(&unique, 123, Some(":1.123")));
        assert!(!owns_registration(&unique, 123, Some(":1.12")));
    }
}
