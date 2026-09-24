"""Check legacy and current window-size restoration in an isolated 2x Xvfb session.

Run with dbus-run-session and xvfb-run as documented in TESTING.md. The probe
uses temporary XDG state and never opens a window on the active desktop.
"""

import ctypes
import os
import pathlib
import subprocess
import sys
import tempfile
import time


def windows():
    x11 = ctypes.CDLL("libX11.so.6")
    x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
    x11.XOpenDisplay.restype = ctypes.c_void_p
    x11.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
    x11.XDefaultRootWindow.restype = ctypes.c_ulong
    x11.XQueryTree.argtypes = [
        ctypes.c_void_p, ctypes.c_ulong,
        ctypes.POINTER(ctypes.c_ulong), ctypes.POINTER(ctypes.c_ulong),
        ctypes.POINTER(ctypes.POINTER(ctypes.c_ulong)), ctypes.POINTER(ctypes.c_uint),
    ]
    x11.XQueryTree.restype = ctypes.c_int
    x11.XGetGeometry.argtypes = [
        ctypes.c_void_p, ctypes.c_ulong, ctypes.POINTER(ctypes.c_ulong),
        ctypes.POINTER(ctypes.c_int), ctypes.POINTER(ctypes.c_int),
        ctypes.POINTER(ctypes.c_uint), ctypes.POINTER(ctypes.c_uint),
        ctypes.POINTER(ctypes.c_uint), ctypes.POINTER(ctypes.c_uint),
    ]
    x11.XGetGeometry.restype = ctypes.c_int
    x11.XFree.argtypes = [ctypes.c_void_p]
    x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
    display = x11.XOpenDisplay(os.environ["DISPLAY"].encode())
    if not display:
        raise RuntimeError("no X display")
    root = x11.XDefaultRootWindow(display)
    parent = ctypes.c_ulong()
    children = ctypes.POINTER(ctypes.c_ulong)()
    count = ctypes.c_uint()
    if not x11.XQueryTree(display, root, ctypes.byref(ctypes.c_ulong()),
                          ctypes.byref(parent), ctypes.byref(children), ctypes.byref(count)):
        raise RuntimeError("XQueryTree failed")
    found = []
    for index in range(count.value):
        window = children[index]
        root_return = ctypes.c_ulong()
        x = ctypes.c_int()
        y = ctypes.c_int()
        width = ctypes.c_uint()
        height = ctypes.c_uint()
        border = ctypes.c_uint()
        depth = ctypes.c_uint()
        if x11.XGetGeometry(display, window, ctypes.byref(root_return),
                            ctypes.byref(x), ctypes.byref(y), ctypes.byref(width),
                            ctypes.byref(height), ctypes.byref(border), ctypes.byref(depth)):
            found.append((width.value, height.value))
    if children:
        x11.XFree(children)
    x11.XCloseDisplay(display)
    return found


def run(binary, header, saved_width, saved_height):
    with tempfile.TemporaryDirectory(prefix="redunar-window-probe-") as root:
        root = pathlib.Path(root)
        for name in ("runtime", "state", "cache", "data", "home"):
            (root / name).mkdir()
        (root / "runtime").chmod(0o700)
        prefs = root / "state" / "redunar"
        prefs.mkdir()
        (prefs / "app-preferences-v1.txt").write_text(
            f"redunar-app-preferences-{header}\n"
            f"close-to-tray=off\nwindow-width={saved_width}\nwindow-height={saved_height}\n"
            "window-maximized=off\nautomatic-updates=off\n"
            + ("window-size-units=logical\n" if header == "v6" else "")
        )
        env = dict(os.environ)
        env.update({
            "HOME": str(root / "home"),
            "XDG_RUNTIME_DIR": str(root / "runtime"),
            "XDG_STATE_HOME": str(root / "state"),
            "XDG_CACHE_HOME": str(root / "cache"),
            "XDG_DATA_HOME": str(root / "data"),
            "GDK_BACKEND": "x11",
            "GDK_SCALE": "2",
            "REDUNAR_UPDATE_SOURCE_URL": "",
            "WEBKIT_DISABLE_DMABUF_RENDERER": "1",
        })
        process = subprocess.Popen([str(binary)], env=env,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
                                   start_new_session=True)
        try:
            deadline = time.monotonic() + 15
            observed = []
            while time.monotonic() < deadline:
                observed = windows()
                if (2560, 1440) in observed:
                    break
                if process.poll() is not None:
                    raise RuntimeError(f"Redunar exited {process.returncode}: {process.stderr.read().decode(errors='replace')[-1500:]}")
                time.sleep(0.25)
            print(f"{header} {saved_width}x{saved_height}: windows={observed}, process_alive={process.poll() is None}")
            if (2560, 1440) not in observed:
                raise RuntimeError("expected 1280x720 logical window did not open at 2x scale")
            if not os.path.samefile(f"/proc/{process.pid}/exe", binary):
                raise RuntimeError("fresh process did not load the selected executable")
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()


if len(sys.argv) != 2:
    raise SystemExit("usage: check-tauri-window-geometry.py /absolute/path/to/redunar-tauri")
for version, width, height in (("v5", 1280, 720), ("v5", 2560, 1440), ("v6", 1280, 720)):
    run(pathlib.Path(sys.argv[1]).resolve(), version, width, height)
