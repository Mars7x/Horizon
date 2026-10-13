"""Spike helper: X11 stand-in windows for gamescope spikes.

Usage: standin.py main|game <appid>
       standin.py overlay <appid> panel|full [NAME=VALUE...]
Sets STEAM_GAME=<appid> and any NAME=VALUE cardinal properties before mapping.
A "game" window animates, so gamescope keeps sending frames."""
import subprocess, sys
import gi
gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
from gi.repository import Gtk, Gdk, GLib

kind, appid = sys.argv[1], sys.argv[2]
props = sys.argv[3:]          # NAME=VALUE cardinal properties set before mapping

screen = Gdk.Screen.get_default()
win = Gtk.Window(title=f"stand-in {kind}")
win.set_decorated(False)
colors = {"main": "#23395d", "game": "#7a4b12"}
label = Gtk.Label()
css = Gtk.CssProvider()

if kind == "overlay":
    shape = props.pop(0)       # panel | full
    visual = screen.get_rgba_visual()
    if visual:
        win.set_visual(visual)
    win.set_app_paintable(True)
    sw, sh = (lambda g: (g.width, g.height))(screen.get_display().get_monitor(0).get_geometry())
    css.load_from_data(b"window { background: rgba(0,0,0,0); }"
                       b" .panel { background: rgba(20,20,28,0.85); }"
                       b" label { font-size: 40px; color: white; }")
    panel = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
    panel.get_style_context().add_class("panel")
    label.set_text("Quick menu\n(overlay)")
    panel.pack_start(label, True, True, 0)
    if shape == "full":
        win.set_default_size(sw, sh)
        row = Gtk.Box()
        spacer = Gtk.Box()
        row.pack_start(spacer, True, True, 0)
        panel.set_size_request(sw // 3, sh)
        row.pack_end(panel, False, False, 0)
        win.add(row)
    else:
        win.set_default_size(sw // 3, sh)
        win.move(sw - sw // 3, 0)
        win.add(panel)
else:
    css.load_from_data(("window { background: %s; } label { font-size: 64px; color: white; }"
                        % colors[kind]).encode())
    win.set_default_size(1280, 720)
    win.add(label)
    if kind == "game":
        count = [0]
        def tick():
            count[0] += 1
            label.set_text(f"Game stand-in ({appid})  {count[0]}")
            return True
        GLib.timeout_add(100, tick)
    label.set_text(f"{kind.capitalize()} stand-in ({appid})")

Gtk.StyleContext.add_provider_for_screen(screen, css, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION)
win.connect("destroy", Gtk.main_quit)
win.realize()
xid = win.get_window().get_xid()
for name, value in [("STEAM_GAME", appid)] + [p.split("=", 1) for p in props]:
    subprocess.run(["xprop", "-id", str(xid), "-f", name, "32c", "-set", name, value])
print(f"{kind} 0x{xid:x} STEAM_GAME={appid} {' '.join(props)}", flush=True)
win.show_all()
Gtk.main()
