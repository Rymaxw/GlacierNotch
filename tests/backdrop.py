#!/usr/bin/env python3
"""Gray backing window for isolated X11 pixel verification."""
import ctypes
import time

x = ctypes.CDLL("libX11.so.6")
x.XOpenDisplay.restype = ctypes.c_void_p
x.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
x.XDefaultRootWindow.restype = ctypes.c_ulong
x.XCreateSimpleWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_int, ctypes.c_uint, ctypes.c_uint, ctypes.c_uint, ctypes.c_ulong, ctypes.c_ulong]
x.XCreateSimpleWindow.restype = ctypes.c_ulong
x.XMapWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
x.XFlush.argtypes = [ctypes.c_void_p]
display = x.XOpenDisplay(b":97")
if not display:
    raise RuntimeError("no display :97")
root = x.XDefaultRootWindow(display)
window = x.XCreateSimpleWindow(display, root, 0, 0, 1280, 720, 0, 0, 0xDDDDDD)
x.XMapWindow(display, window)
x.XFlush(display)
print("BACKDROP_READY", flush=True)
while True:
    time.sleep(60)
