"""Shared production renderer setup for isolated WebKit fixture processes."""
import os


def configure_webkit():
    # Match configure_webkit_renderer in the native host, retaining an explicit
    # override for deliberate renderer investigations. Never disable sandboxing.
    os.environ.setdefault("WEBKIT_DISABLE_DMABUF_RENDERER", "1")
