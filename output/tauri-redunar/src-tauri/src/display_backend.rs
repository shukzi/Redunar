//! Desktop backend preference shared by manual and Steam background startup.
use std::ffi::OsStr;

pub(crate) fn configure() {
    if let Some(backends) = preferred_backends(
        std::env::var_os("XDG_SESSION_TYPE").as_deref(),
        std::env::var_os("WAYLAND_DISPLAY").as_deref(),
        std::env::var_os("GDK_BACKEND").as_deref(),
    ) {
        // Run before GTK initialization and before service threads. GDK tries
        // these in order, including X11 when the Wayland connection fails.
        // This process-local preference does not alter the environment passed
        // to a game or helper. An explicit GDK_BACKEND remains authoritative.
        gtk::gdk::set_allowed_backends(backends);
    }
}

fn preferred_backends(
    session: Option<&OsStr>,
    wayland_display: Option<&OsStr>,
    explicit: Option<&OsStr>,
) -> Option<&'static str> {
    if explicit.is_some() {
        return None;
    }
    if session == Some(OsStr::new("x11")) {
        return Some("x11");
    }
    if session == Some(OsStr::new("wayland"))
        || wayland_display.is_some_and(|display| !display.is_empty())
    {
        Some("wayland,x11")
    } else {
        None
    }
}

pub(crate) fn record_selected() {
    use gtk::prelude::*;
    if !redunar_daemon::diagnostic_log::enabled() {
        return;
    }
    let backend = gtk::gdk::Display::default().map(|display| match display.type_().name() {
        "GdkWaylandDisplay" => "wayland",
        "GdkX11Display" => "x11",
        _ => "other",
    });
    // Log the selected backend, never the private display/socket address.
    redunar_daemon::diagnostic_log::log(&format!(
        "ui display_backend={}",
        backend.unwrap_or("unavailable")
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wayland_sessions_prefer_native_with_x11_fallback() {
        for display in [None, Some(OsStr::new("")), Some(OsStr::new("wayland-1"))] {
            assert_eq!(
                preferred_backends(Some(OsStr::new("wayland")), display, None),
                Some("wayland,x11")
            );
        }
    }

    #[test]
    fn a_wayland_display_handles_launchers_without_a_session_type() {
        assert_eq!(
            preferred_backends(None, Some(OsStr::new("wayland-1")), None),
            Some("wayland,x11")
        );
        assert_eq!(preferred_backends(None, Some(OsStr::new("")), None), None);
        assert_eq!(preferred_backends(None, None, None), None);
    }

    #[test]
    fn an_explicit_x11_session_keeps_its_desktop_even_with_a_nested_wayland_display() {
        assert_eq!(
            preferred_backends(Some(OsStr::new("x11")), Some(OsStr::new("wayland-1")), None),
            Some("x11")
        );
    }

    #[test]
    fn explicit_backend_lists_and_empty_overrides_are_preserved() {
        for value in ["x11", "wayland", "x11,wayland", "wayland,x11", "", "help"] {
            assert_eq!(
                preferred_backends(
                    Some(OsStr::new("wayland")),
                    Some(OsStr::new("wayland-1")),
                    Some(OsStr::new(value))
                ),
                None
            );
        }
    }
}
