"""Send real X11 input only inside check-steam-background's private display."""
import ctypes
import os
import sys


def main():
    if os.environ.get("REDUNAR_PRIVATE_DESKTOP_FIXTURE") != "1":
        raise SystemExit("Run through the private Steam desktop fixture")
    x11 = ctypes.CDLL("libX11.so.6")
    xtest = ctypes.CDLL("libXtst.so.6")
    x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
    x11.XOpenDisplay.restype = ctypes.c_void_p
    x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
    x11.XKeysymToKeycode.restype = ctypes.c_ubyte
    x11.XFlush.argtypes = [ctypes.c_void_p]
    x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
    for name in ("XTestFakeKeyEvent", "XTestFakeButtonEvent"):
        function = getattr(xtest, name)
        function.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
        function.restype = ctypes.c_int
    xtest.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
    xtest.XTestFakeMotionEvent.restype = ctypes.c_int
    display = x11.XOpenDisplay(None)
    if not display:
        raise RuntimeError("private X display unavailable")
    try:
        if sys.argv[1] == "key":
            key = x11.XKeysymToKeycode(display, ord("a"))
            assert key
            function, value = xtest.XTestFakeKeyEvent, key
        elif sys.argv[1] == "click":
            assert xtest.XTestFakeMotionEvent(display, -1, int(sys.argv[2]), int(sys.argv[3]), 0)
            function, value = xtest.XTestFakeButtonEvent, 1
        else:
            raise RuntimeError("Unknown fixture input")
        for pressed in (1, 0):
            assert function(display, value, pressed, 0)
        x11.XFlush(display)
    finally:
        x11.XCloseDisplay(display)


if __name__ == "__main__":
    main()
