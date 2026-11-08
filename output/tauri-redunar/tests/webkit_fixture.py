"""Shared production renderer setup for isolated WebKit fixture processes."""
import os


def configure_webkit():
    # Match configure_webkit_renderer in the native host, retaining an explicit
    # override for deliberate renderer investigations. Never disable sandboxing.
    os.environ.setdefault("WEBKIT_DISABLE_DMABUF_RENDERER", "1")


def configure_webview(view):
    if os.environ.get("GDK_BACKEND") == "broadway":
        from gi.repository import Gdk, Gtk
        # Broadway has no monitor DPI until a client connects. WebKit otherwise
        # derives a negative page scale; fix only this fixture process's display.
        Gtk.Settings.get_default().set_property("gtk-xft-dpi", 96 * 1024)
        Gdk.Screen.get_default().set_resolution(96)
    # Match the main view's restored native smooth scrolling; the UI does not
    # intercept wheel input. Never change GTK/system preferences.
    view.get_settings().set_enable_smooth_scrolling(True)
