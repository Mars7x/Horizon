#!/usr/bin/env bash
# Spike 6 (docs/GAMESCOPE_SESSION.md): the power key. The power button's
# input devices are root:input 0660 with no per-user access, so Horizon
# can't read them directly. Does embedded gamescope, which reads every
# keyboard through libinput, deliver the power key to the focused X11 window
# instead? Can a short press be told from a long one?
#
# Runs on the host with gamescope layered. Changes nothing in Horizon.
#
# Safety: logind powers off on the power key unless someone holds a
# handle-power-key block inhibitor. Stay logged in to GNOME (it holds one,
# and its power-button action here is "nothing"), switch to a text console
# (Ctrl+Alt+F3), log in, and run:
#
#   bash ~/Projects/Horizon/scripts/spikes/gamescope-spike6.sh
#
# The script refuses to start without a handle-power-key block inhibitor.
# Never hold the button for 4 seconds or more: PC hardware then forces a
# power-off that nothing can block.
#
# The screen asks for one brief press, then one hold of about 2 seconds. A
# full-screen X11 window (STEAM_GAME=0x7F000000, the base layer) records
# every key it receives. About 1 minute.
#
# Output: ~/horizon-spikes/spike6-<time>/.
set -uo pipefail

MAIN_APPID=$((0x7F000000))
PHASE=${PHASE:-20}

if [[ "${1:-}" != --inner ]]; then
    for tool in gamescope xprop python3 systemd-inhibit; do
        command -v "$tool" >/dev/null || { echo "Missing: $tool" >&2; exit 1; }
    done
    if [[ -n "${WAYLAND_DISPLAY:-}${DISPLAY:-}" ]]; then
        echo "Run this from a text console (Ctrl+Alt+F3), not inside the desktop." >&2
        exit 1
    fi
    inhibitors=$(systemd-inhibit --list --no-pager 2>/dev/null | grep -E 'handle-power-key' | grep -w block)
    if [[ -z "$inhibitors" ]]; then
        echo "No handle-power-key block inhibitor is held, so the power key would power off." >&2
        echo "Stay logged in to GNOME on its console and run this from another one." >&2
        exit 1
    fi
    OUT="$HOME/horizon-spikes/spike6-$(date +%Y%m%d-%H%M%S)"
    mkdir -p "$OUT"
    {
        echo "mode: embedded"
        echo "gamescope: $(rpm -q gamescope 2>/dev/null || echo unknown)"
        echo "inhibitors holding the power key:"
        echo "$inhibitors" | sed 's/^/   /'
        echo "logind HandlePowerKey: $(busctl get-property org.freedesktop.login1 /org/freedesktop/login1 \
            org.freedesktop.login1.Manager HandlePowerKey 2>&1)"
    } > "$OUT/steps.log"
    gamescope -e --backend drm -- "$0" --inner "$OUT" > "$OUT/gamescope.log" 2>&1
    echo "gamescope exit: $?" >> "$OUT/steps.log"
    echo "Results: $OUT"
    exit 0
fi

# ---- Inside gamescope ------------------------------------------------------
OUT=$2
LOG="$OUT/steps.log"

cat > "$OUT/keys.py" <<'EOF'
import subprocess, sys, time
import gi
gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
from gi.repository import Gtk, Gdk, GLib

appid, phase, log_path = sys.argv[1], int(sys.argv[2]), sys.argv[3]
log = open(log_path, "a", buffering=1)
start = time.monotonic()
down = {}

def stamp():
    return f"{time.monotonic() - start:7.3f}s"

win = Gtk.Window(title="Power key test")
css = Gtk.CssProvider()
css.load_from_data(b"window { background: #1d2b3a; } label { font-size: 56px; color: white; }")
Gtk.StyleContext.add_provider_for_screen(Gdk.Screen.get_default(), css,
                                         Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION)
label = Gtk.Label()
label.set_justify(Gtk.Justification.CENTER)
win.add(label)
win.fullscreen()

def on_key(_w, event):
    name = Gdk.keyval_name(event.keyval) or f"keyval {event.keyval}"
    kind = "press" if event.type == Gdk.EventType.KEY_PRESS else "release"
    extra = ""
    if kind == "press":
        down.setdefault(event.hardware_keycode, time.monotonic())
    else:
        t = down.pop(event.hardware_keycode, None)
        if t is not None:
            extra = f" held {time.monotonic() - t:.2f}s"
    log.write(f"{stamp()} {kind} {name} (keycode {event.hardware_keycode}){extra}\n")
    label.set_text(f"{prompt[0]}\n\nlast key: {name} {kind}{extra}")
    return True

win.connect("key-press-event", on_key)
win.connect("key-release-event", on_key)
win.connect("destroy", Gtk.main_quit)

prompt = [""]
def set_prompt(text):
    prompt[0] = text
    label.set_text(text)
    log.write(f"{stamp()} prompt: {text.splitlines()[0]}\n")
    return False

win.realize()
xid = win.get_window().get_xid()
subprocess.run(["xprop", "-id", str(xid), "-f", "STEAM_GAME", "32c", "-set", "STEAM_GAME", appid])
win.show_all()
set_prompt("Press the power button once, briefly.")
GLib.timeout_add_seconds(phase, set_prompt,
                         "Now hold the power button for about 2 seconds, then let go.\n(Never 4 seconds or more.)")
GLib.timeout_add_seconds(2 * phase, set_prompt, "Done. Don't press anything.")
GLib.timeout_add_seconds(2 * phase + 3, Gtk.main_quit)
Gtk.main()
EOF

xprop -root -f GAMESCOPECTRL_BASELAYER_APPID 32c -set GAMESCOPECTRL_BASELAYER_APPID "$MAIN_APPID"
GDK_BACKEND=x11 python3 -I "$OUT/keys.py" "$MAIN_APPID" "$PHASE" "$OUT/keys.log" >> "$OUT/window.log" 2>&1
{
    echo "keys received by the window:"
    sed 's/^/   /' "$OUT/keys.log"
    echo "gamescope log lines about keys or power:"
    sed 's/\x1b\[[0-9;]*m//g' "$OUT/gamescope.log" | grep -iE 'power|key|input' \
        | grep -viE 'xkbcomp|keymap|keycode [0-9]+ |Warning|> ' | sed 's/^/   /' | head -20
    echo "power-key inhibitors at the end:"
    systemd-inhibit --list --no-pager 2>/dev/null | grep handle-power-key | sed 's/^/   /'
} >> "$LOG"
