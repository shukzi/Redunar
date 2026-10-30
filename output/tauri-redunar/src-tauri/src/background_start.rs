//! Steam startup remains reachable while respecting the saved tray preference.
use std::ffi::OsString;
use std::sync::{Arc, Mutex, MutexGuard};
use tauri::Manager;

#[derive(Clone)]
pub(crate) struct State(Arc<Mutex<Ownership>>);

struct Ownership {
    auto_exit: bool,
    retained: bool,
    next_close: u64,
    pending_close: Option<u64>,
}

impl State {
    pub fn new(automatic: bool) -> Self {
        Self(Arc::new(Mutex::new(Ownership {
            auto_exit: automatic,
            retained: false,
            next_close: 0,
            pending_close: None,
        })))
    }

    fn ownership(&self) -> MutexGuard<'_, Ownership> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn may_auto_exit(&self) -> bool {
        let state = self.ownership();
        state.auto_exit && !state.retained
    }

    fn retain(&self) {
        let mut state = self.ownership();
        state.retained = true;
        state.pending_close = None;
    }

    pub fn begin_close(&self) -> u64 {
        let mut state = self.ownership();
        // Closing the window relinquishes foreground ownership, including an
        // app started manually. Keep the live game owned until natural cleanup.
        state.auto_exit = true;
        state.retained = false;
        state.next_close = state.next_close.saturating_add(1);
        state.pending_close = Some(state.next_close);
        state.next_close
    }

    pub fn close_pending(&self) -> bool {
        self.ownership().pending_close.is_some()
    }

    pub fn close_is_current(&self, ticket: u64) -> bool {
        self.ownership().pending_close == Some(ticket)
    }

    pub fn finish_close(&self, ticket: u64) {
        let mut state = self.ownership();
        if state.pending_close == Some(ticket) {
            state.pending_close = None;
        }
    }
}

pub(crate) fn retain_window(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<State>() {
        state.retain();
    }
}

#[cfg(target_os = "linux")]
fn observe_input(widget: &impl gtk::prelude::IsA<gtk::Widget>, state: State) {
    use gtk::prelude::*;
    // Focus can come from the compositor at startup or when a game closes.
    // Observe actual input without consuming it, including WebKit-handled keys.
    // Capture only ownership state: a widget/AppHandle cycle would retain the app.
    widget.connect_event_after(move |_, event| {
        if is_user_input(event.event_type()) {
            state.retain();
        }
    });
}

#[cfg(target_os = "linux")]
fn is_user_input(event: gtk::gdk::EventType) -> bool {
    use gtk::gdk::EventType;
    matches!(
        event,
        EventType::ButtonPress | EventType::KeyPress | EventType::TouchBegin | EventType::Scroll
    )
}

fn install_input_tracking(app: &tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let window = app
            .get_webview_window("main")
            .ok_or("Main window unavailable")?;
        let state = app.state::<State>().inner().clone();
        observe_input(
            &window.gtk_window().map_err(|error| error.to_string())?,
            state.clone(),
        );
        window
            .with_webview(move |webview| observe_input(&webview.inner(), state))
            .map_err(|error| error.to_string())
    }
    #[cfg(not(target_os = "linux"))]
    Err("Native window input tracking is unavailable".into())
}

pub fn requested(arguments: impl IntoIterator<Item = OsString>) -> bool {
    let mut arguments = arguments.into_iter();
    arguments.next().as_deref()
        == Some(std::ffi::OsStr::new(
            redunar_platform::STEAM_BACKGROUND_ARGUMENT,
        ))
        && arguments.next().is_none()
}

pub(crate) fn track_window_input(app: &tauri::AppHandle) {
    if install_input_tracking(app).is_err() {
        // Keep the owner reachable if deliberate interaction cannot be observed.
        retain_window(app);
        redunar_daemon::log_op!("Window input tracking unavailable; background exit disabled");
    }
}

pub fn prepare_window(app: &tauri::AppHandle) {
    track_window_input(app);
    // Automatic Steam startup gets a temporary tray entry even when closing a
    // manually opened window should quit. Never persist this startup exception.
    // Keeping the main window unmapped avoids compositor-driven activation.
    if crate::tray::set_enabled(app, true).is_err() {
        show_minimized(app);
        return;
    }
    #[cfg(target_os = "linux")]
    {
        let app = app.clone();
        gtk::glib::timeout_add_local_once(std::time::Duration::from_secs(1), move || {
            // Retention after an input-tracking failure must not strand a hidden
            // window. Skip only a window already exposed to the user.
            if !crate::tray::registered(&app)
                && app
                    .get_webview_window("main")
                    .is_some_and(|window| !window.is_visible().unwrap_or(true))
            {
                show_minimized(&app);
            }
        });
    }
    #[cfg(not(target_os = "linux"))]
    show_minimized(app);
}

fn show_minimized(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    #[cfg(target_os = "linux")]
    if let Ok(native) = window.gtk_window() {
        use gtk::prelude::*;
        // Set the initial map hints before showing the window. Requesting an
        // unfocused, iconic map leaves Steam/game activation with the desktop.
        native.set_focus_on_map(false);
        native.iconify();
    }
    #[cfg(not(target_os = "linux"))]
    let _ = window.minimize();
    if window.show().is_err() {
        crate::tray::show_window(app);
    } else {
        crate::tray::remove_if_disabled(app);
    }
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

    #[test]
    fn manual_owners_and_opened_automatic_windows_are_retained() {
        let manual = State::new(false);
        assert!(!manual.may_auto_exit());
        let automatic = State::new(true);
        assert!(automatic.may_auto_exit());
        automatic.retain();
        assert!(!automatic.may_auto_exit());
        automatic.retain();
        assert!(!automatic.may_auto_exit());
    }

    #[test]
    fn closing_a_game_window_rearms_exit_for_manual_and_opened_owners() {
        for automatic in [false, true] {
            let state = State::new(automatic);
            state.retain();
            assert!(!state.may_auto_exit());
            let ticket = state.begin_close();
            assert!(state.may_auto_exit());
            assert!(state.close_is_current(ticket));
            state.finish_close(ticket);
            assert!(!state.close_pending());
            assert!(state.may_auto_exit());
        }
    }

    #[test]
    fn open_or_input_cancels_pending_close_and_duplicate_close_supersedes_it() {
        let state = State::new(false);
        let first = state.begin_close();
        let second = state.begin_close();
        assert!(!state.close_is_current(first));
        assert!(state.close_is_current(second));
        state.finish_close(first);
        assert!(state.close_is_current(second));
        state.retain();
        assert!(!state.close_is_current(second));
        assert!(!state.may_auto_exit());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn compositor_focus_and_passive_pointer_events_are_not_user_input() {
        use gtk::gdk::EventType;
        for event in [
            EventType::FocusChange,
            EventType::WindowState,
            EventType::Map,
            EventType::MotionNotify,
            EventType::EnterNotify,
            EventType::LeaveNotify,
        ] {
            assert!(!is_user_input(event));
        }
        for event in [
            EventType::ButtonPress,
            EventType::KeyPress,
            EventType::TouchBegin,
            EventType::Scroll,
        ] {
            assert!(is_user_input(event));
        }
    }
}
