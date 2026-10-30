//! Native window parking with a simulated live game, never a real capture.
use super::{
    backend, background_start, background_start::State, exit_probe, host::Host, tray,
    window_lifecycle,
};
use tauri::Manager;

pub fn run(app: tauri::AppHandle, host: Option<Host>, cancel: bool) {
    // A native service callback says the fixture game is still live. Neither
    // closing nor reopening invokes session end or owns a process.
    window_lifecycle::close_for_game(&app, |_| false);
    // Model GTK's trailing event-after observer for the close button itself.
    // It precedes the deferred close transition; later input still cancels it.
    background_start::retain_window(&app);
    if cancel {
        let handle = app.clone();
        gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(100), move || {
            tray::show_window(&handle);
        });
    }
    gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(1_400), move || {
        let window = app.get_webview_window("main").unwrap();
        if cancel || host.is_none() {
            assert!(
                window.is_visible().unwrap(),
                "cancel/unavailable tray keeps window reachable"
            );
            assert!(!app.state::<State>().may_auto_exit());
            assert!(!app.state::<State>().close_pending());
            println!(
                "PASS: canceled/unavailable-tray close keeps the live session owner reachable"
            );
            exit_probe(&app, host);
            return;
        }
        assert!(
            !window.is_visible().unwrap(),
            "closing parks the live game owner"
        );
        assert!(tray::registered(&app));
        assert!(app.state::<State>().may_auto_exit());
        assert!(!app.state::<State>().close_pending());
        println!(
            "PASS: closing re-arms background exit and hides only with usable tray registration"
        );
        tray::show_window(&app);
        gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(200), move || {
            assert!(app
                .get_webview_window("main")
                .unwrap()
                .is_visible()
                .unwrap());
            assert!(
                !app.state::<State>().may_auto_exit(),
                "reopening cancels exit"
            );
            assert_eq!(
                tray::has_item(&app),
                backend::service().app_preferences().unwrap().close_to_tray
            );
            window_lifecycle::close_for_game(&app, |_| false);
            background_start::retain_window(&app);
            gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(1_400), move || {
                assert!(!app
                    .get_webview_window("main")
                    .unwrap()
                    .is_visible()
                    .unwrap());
                assert!(app.state::<State>().may_auto_exit());
                println!("PASS: Open restores preference; closing again re-arms automatic exit");
                exit_probe(&app, host);
            });
        });
    });
}
