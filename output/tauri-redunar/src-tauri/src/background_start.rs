//! Steam starts the same native app with a hidden, reachable main window.
use std::ffi::OsString;

pub fn requested(arguments: impl IntoIterator<Item = OsString>) -> bool {
    let mut arguments = arguments.into_iter();
    arguments.next().as_deref()
        == Some(std::ffi::OsStr::new(
            redunar_platform::STEAM_BACKGROUND_ARGUMENT,
        ))
        && arguments.next().is_none()
}

pub fn prepare_window(app: &tauri::AppHandle) {
    // This is a temporary startup icon, not a change to Close to tray. Never
    // leave an invisible owner on desktops without a working tray provider.
    if crate::tray::set_enabled(app, true).is_err() {
        crate::tray::show_window(app);
        return;
    }
    #[cfg(target_os = "linux")]
    {
        let app = app.clone();
        gtk::glib::timeout_add_local_once(std::time::Duration::from_secs(1), move || {
            if !crate::tray::registered() {
                crate::tray::show_window(&app);
            }
        });
    }
    #[cfg(not(target_os = "linux"))]
    crate::tray::show_window(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_exact_background_invocation_hides_startup() {
        assert!(requested([OsString::from(
            redunar_platform::STEAM_BACKGROUND_ARGUMENT
        )]));
        assert!(!requested([]));
        assert!(!requested([OsString::from("--other")]));
        assert!(!requested([
            OsString::from(redunar_platform::STEAM_BACKGROUND_ARGUMENT),
            OsString::from("unexpected")
        ]));
    }
}
