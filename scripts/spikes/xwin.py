"""Spike helper: list mapped top-level X11 windows, or tag windows with STEAM_GAME.

Usage: xwin.py list | xwin.py tag <appid> <0xwindow>... | xwin.py close|kill <0xwindow>...
"close" asks politely (WM_DELETE_WINDOW); "kill" drops the client (XKillClient).
Uses libX11 through ctypes, so the host needs nothing extra."""
import ctypes, ctypes.util, sys

x = ctypes.CDLL(ctypes.util.find_library("X11") or "libX11.so.6")
W = ctypes.c_ulong
x.XOpenDisplay.restype = ctypes.c_void_p
x.XOpenDisplay.argtypes = [ctypes.c_char_p]
x.XDefaultRootWindow.restype = W
x.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
x.XInternAtom.restype = W
x.XInternAtom.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int]
x.XQueryTree.argtypes = [ctypes.c_void_p, W, ctypes.POINTER(W), ctypes.POINTER(W),
                         ctypes.POINTER(ctypes.POINTER(W)), ctypes.POINTER(ctypes.c_uint)]
x.XFetchName.argtypes = [ctypes.c_void_p, W, ctypes.POINTER(ctypes.c_char_p)]
x.XGetWindowProperty.argtypes = [ctypes.c_void_p, W, W, ctypes.c_long, ctypes.c_long, ctypes.c_int,
                                 W, ctypes.POINTER(W), ctypes.POINTER(ctypes.c_int),
                                 ctypes.POINTER(ctypes.c_ulong), ctypes.POINTER(ctypes.c_ulong),
                                 ctypes.POINTER(ctypes.c_void_p)]
x.XChangeProperty.argtypes = [ctypes.c_void_p, W, W, W, ctypes.c_int, ctypes.c_int,
                              ctypes.c_void_p, ctypes.c_int]
x.XFlush.argtypes = [ctypes.c_void_p]
x.XKillClient.argtypes = [ctypes.c_void_p, W]

class ClientMessage(ctypes.Structure):
    _fields_ = [("type", ctypes.c_int), ("serial", ctypes.c_ulong), ("send_event", ctypes.c_int),
                ("display", ctypes.c_void_p), ("window", W), ("message_type", W),
                ("format", ctypes.c_int), ("l", ctypes.c_long * 5)]

class XEvent(ctypes.Union):
    _fields_ = [("xclient", ClientMessage), ("pad", ctypes.c_long * 24)]

x.XSendEvent.argtypes = [ctypes.c_void_p, W, ctypes.c_int, ctypes.c_long, ctypes.POINTER(XEvent)]

class Attrs(ctypes.Structure):
    _fields_ = [("x", ctypes.c_int), ("y", ctypes.c_int), ("width", ctypes.c_int),
                ("height", ctypes.c_int), ("border_width", ctypes.c_int), ("depth", ctypes.c_int),
                ("visual", ctypes.c_void_p), ("root", W), ("c_class", ctypes.c_int),
                ("bit_gravity", ctypes.c_int), ("win_gravity", ctypes.c_int),
                ("backing_store", ctypes.c_int), ("backing_planes", ctypes.c_ulong),
                ("backing_pixel", ctypes.c_ulong), ("save_under", ctypes.c_int),
                ("colormap", W), ("map_installed", ctypes.c_int), ("map_state", ctypes.c_int),
                ("all_event_masks", ctypes.c_long), ("your_event_mask", ctypes.c_long),
                ("do_not_propagate_mask", ctypes.c_long), ("override_redirect", ctypes.c_int),
                ("screen", ctypes.c_void_p)]
x.XGetWindowAttributes.argtypes = [ctypes.c_void_p, W, ctypes.POINTER(Attrs)]

d = x.XOpenDisplay(None)
if not d:
    sys.exit("no X display")
root = x.XDefaultRootWindow(d)
STEAM_GAME = x.XInternAtom(d, b"STEAM_GAME", 0)
CARDINAL = x.XInternAtom(d, b"CARDINAL", 0)

def steam_game(w):
    t, f, n, a, p = W(), ctypes.c_int(), ctypes.c_ulong(), ctypes.c_ulong(), ctypes.c_void_p()
    if x.XGetWindowProperty(d, w, STEAM_GAME, 0, 1, 0, CARDINAL, ctypes.byref(t), ctypes.byref(f),
                            ctypes.byref(n), ctypes.byref(a), ctypes.byref(p)) == 0 and n.value:
        return ctypes.cast(p, ctypes.POINTER(ctypes.c_ulong))[0]
    return None

if sys.argv[1] == "list":
    r, parent, kids, count = W(), W(), ctypes.POINTER(W)(), ctypes.c_uint()
    x.XQueryTree(d, root, ctypes.byref(r), ctypes.byref(parent), ctypes.byref(kids), ctypes.byref(count))
    for i in range(count.value):
        w = kids[i]
        at = Attrs()
        x.XGetWindowAttributes(d, w, ctypes.byref(at))
        if at.map_state != 2 or at.width <= 1 or at.height <= 1:  # IsViewable, real size
            continue
        name = ctypes.c_char_p()
        x.XFetchName(d, w, ctypes.byref(name))
        title = (name.value or b"").decode(errors="replace")
        print(f"0x{w:x} {at.width}x{at.height} override={at.override_redirect} "
              f"steam_game={steam_game(w)} title={title!r}")
elif sys.argv[1] == "tag":
    value = ctypes.c_ulong(int(sys.argv[2]))
    for w in sys.argv[3:]:
        x.XChangeProperty(d, int(w, 16), STEAM_GAME, CARDINAL, 32, 0, ctypes.byref(value), 1)
    x.XFlush(d)
elif sys.argv[1] == "close":
    WM_PROTOCOLS = x.XInternAtom(d, b"WM_PROTOCOLS", 0)
    WM_DELETE = x.XInternAtom(d, b"WM_DELETE_WINDOW", 0)
    for w in sys.argv[2:]:
        ev = XEvent()
        ev.xclient.type = 33  # ClientMessage
        ev.xclient.window = int(w, 16)
        ev.xclient.message_type = WM_PROTOCOLS
        ev.xclient.format = 32
        ev.xclient.l[0] = WM_DELETE
        x.XSendEvent(d, int(w, 16), 0, 0, ctypes.byref(ev))
    x.XFlush(d)
elif sys.argv[1] == "kill":
    for w in sys.argv[2:]:
        x.XKillClient(d, int(w, 16))
    x.XFlush(d)
