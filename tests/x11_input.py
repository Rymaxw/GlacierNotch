"""XTest input confined to the display explicitly supplied by the QA runner."""
import ctypes
import time


class XInput:
    def __init__(self, display):
        assert display != ":0", "QA input must never target Max's desktop"
        self.x = ctypes.CDLL("libX11.so.6")
        self.t = ctypes.CDLL("libXtst.so.6")
        p, u, i = ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int
        signatures = {
            "XOpenDisplay": ([ctypes.c_char_p], p),
            "XDefaultRootWindow": ([p], u),
            "XFlush": ([p], i),
            "XCloseDisplay": ([p], i),
            "XQueryPointer": ([p, u] + [p] * 7, i),
            "XGetInputFocus": ([p, p, p], i),
            "XSetInputFocus": ([p, u, i, u], i),
            "XStringToKeysym": ([ctypes.c_char_p], u),
            "XKeysymToKeycode": ([p, u], ctypes.c_ubyte),
        }
        for name, (args, result) in signatures.items():
            fn = getattr(self.x, name)
            fn.argtypes, fn.restype = args, result
        for name, args in {
            "XTestFakeMotionEvent": [p, i, i, i, u],
            "XTestFakeButtonEvent": [p, ctypes.c_uint, i, u],
            "XTestFakeKeyEvent": [p, ctypes.c_uint, i, u],
        }.items():
            fn = getattr(self.t, name)
            fn.argtypes, fn.restype = args, i
        self.handle = self.x.XOpenDisplay(display.encode())
        assert self.handle, "cannot open isolated display"
        self.root = self.x.XDefaultRootWindow(self.handle)

    def close(self):
        if self.handle:
            self.x.XCloseDisplay(self.handle)
            self.handle = None

    def move(self, px, py, settle=0.42):
        assert self.t.XTestFakeMotionEvent(self.handle, 0, int(px), int(py), 0)
        self.x.XFlush(self.handle)
        time.sleep(settle)
        data = [ctypes.c_ulong(), ctypes.c_ulong(), ctypes.c_int(), ctypes.c_int(), ctypes.c_int(), ctypes.c_int(), ctypes.c_uint()]
        assert self.x.XQueryPointer(self.handle, self.root, *(ctypes.byref(value) for value in data))
        return data[1].value

    def button(self, down, button=1, settle=0.08):
        assert self.t.XTestFakeButtonEvent(self.handle, button, int(down), 0)
        self.x.XFlush(self.handle)
        time.sleep(settle)

    def click(self, px, py, settle=0.4):
        self.move(px, py, settle=0.06)
        self.button(True, settle=0.05)
        self.button(False, settle=settle)

    def wheel(self, px, py, up=True):
        self.move(px, py, settle=0.05)
        self.button(True, 4 if up else 5, settle=0.02)
        self.button(False, 4 if up else 5, settle=0.3)

    def focus(self, window):
        self.x.XSetInputFocus(self.handle, window, 1, 0)
        self.x.XFlush(self.handle)
        time.sleep(0.1)

    def get_focus(self):
        focus = ctypes.c_ulong()
        revert_to = ctypes.c_int()
        self.x.XGetInputFocus(self.handle, ctypes.byref(focus), ctypes.byref(revert_to))
        return focus.value

    def _key_event(self, name, down):
        symbol = self.x.XStringToKeysym(name.encode())
        code = self.x.XKeysymToKeycode(self.handle, symbol)
        assert code, f"unknown X11 key {name}"
        assert self.t.XTestFakeKeyEvent(self.handle, code, int(down), 0)

    def key(self, name, modifiers=(), settle=0.2):
        for modifier in modifiers:
            self._key_event(modifier, True)
        self._key_event(name, True)
        self._key_event(name, False)
        for modifier in reversed(modifiers):
            self._key_event(modifier, False)
        self.x.XFlush(self.handle)
        time.sleep(settle)

    def type_text(self, text):
        names = {" ": "space", "\n": "Return", ".": "period", "-": "minus"}
        for char in text:
            self.key(names.get(char, char.lower()), ("Shift_L",) if char.isupper() else (), settle=0.01)
        time.sleep(0.15)
