#!/usr/bin/env bash
# Overlay spike (the open part of spike 1 in docs/GAMESCOPE_SESSION.md): can
# a window be drawn over a running game, as Horizon's quick menu would be?
# Which window property does it take, does the game stay visible beneath it,
# and which window gets input?
#
# Runs on the host with gamescope layered. Changes nothing in Horizon. X11
# stand-ins play every part: the main app (STEAM_GAME=7690000, as in
# spike 3), a game (STEAM_GAME=4242, animated so gamescope keeps sending
# frames) and the overlay, a translucent panel on the right like the quick
# menu.
#
#   bash scripts/spikes/gamescope-spike-overlay.sh
#
# From a desktop gamescope runs nested; from a text console, embedded.
# Embedded, stay on that console until "Results:" (about 5 minutes):
# switching away pauses gamescope and the run stops partway.
#
# Rounds (overlay properties are set before the window is mapped):
#   steam-overlay        STEAM_OVERLAY=1, STEAM_GAME=main app
#   steam-overlay-769    STEAM_OVERLAY=1, STEAM_GAME=769 (Steam's own ID)
#   external-overlay     GAMESCOPE_EXTERNAL_OVERLAY=1, STEAM_GAME=main app
#   steam-overlay-full   as steam-overlay, but a full-screen window that is
#                        transparent except for the panel
#   steam-overlay-input  as steam-overlay, plus STEAM_INPUT_FOCUS=1
#   steam-overlay-full-input   full-screen, plus STEAM_INPUT_FOCUS=1
#   external-overlay-full      GAMESCOPE_EXTERNAL_OVERLAY=1, full-screen
#
# Output: ~/horizon-spikes/spike-overlay-<time>/.
set -uo pipefail

MAIN_APPID=7690000
GAME_APPID=4242
WAIT=${WAIT:-4}
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROUNDS=(
    "steam-overlay:STEAM_OVERLAY:$MAIN_APPID:panel:0"
    "steam-overlay-769:STEAM_OVERLAY:769:panel:0"
    "external-overlay:GAMESCOPE_EXTERNAL_OVERLAY:$MAIN_APPID:panel:0"
    "steam-overlay-full:STEAM_OVERLAY:$MAIN_APPID:full:0"
    "steam-overlay-input:STEAM_OVERLAY:$MAIN_APPID:panel:1"
    "steam-overlay-full-input:STEAM_OVERLAY:$MAIN_APPID:full:1"
    "external-overlay-full:GAMESCOPE_EXTERNAL_OVERLAY:$MAIN_APPID:full:0"
)

if [[ "${1:-}" != --inner ]]; then
    for tool in gamescope gamescopectl xprop python3 gst-launch-1.0; do
        command -v "$tool" >/dev/null || { echo "Missing: $tool" >&2; exit 1; }
    done
    OUT="$HOME/horizon-spikes/spike-overlay-$(date +%Y%m%d-%H%M%S)"
    mkdir -p "$OUT"
    args=(-e)
    if [[ -n "${WAYLAND_DISPLAY:-}${DISPLAY:-}" ]]; then
        mode=nested
        args+=(-W 1920 -H 1080 -w 1920 -h 1080)
    else
        mode=embedded
        args+=(--backend drm)
    fi
    {
        echo "mode: $mode"
        echo "gamescope: $(rpm -q gamescope 2>/dev/null || echo unknown)"
        echo "command: gamescope ${args[*]} -- $0 --inner"
    } > "$OUT/steps.log"
    gamescope "${args[@]}" -- "$0" --inner "$OUT" > "$OUT/gamescope.log" 2>&1
    echo "gamescope exit: $?" >> "$OUT/steps.log"
    echo "Results: $OUT"
    exit 0
fi

# ---- Inside gamescope ------------------------------------------------------
OUT=$2
LOG="$OUT/steps.log"
GSLOG="$OUT/gamescope.log"
step=0

log() { echo "[$(date +%T)] $*" >> "$LOG"; }

windows() { python3 -I "$HERE/xwin.py" list 2>&1; }

focus_info() {
    local before
    before=$(wc -l < "$GSLOG")
    WAYLAND_DISPLAY="$GAMESCOPE_WAYLAND_DISPLAY" gamescopectl focus_info >/dev/null 2>&1
    sleep 1
    tail -n +"$((before + 1))" "$GSLOG" | sed 's/\x1b\[[0-9;]*m//g' \
        | grep -E 'focus: +(Global focus|Global input focus|Global keyboard focus|Global overlay|  Override)' \
        | sed 's/.*focus: */   /' >> "$LOG"
}

frame() {
    local node
    node=$(sed -n 's/.*stream available on node ID: \([0-9]*\).*/\1/p' "$GSLOG" | tail -1)
    if [[ -n "$node" ]] && timeout 10 gst-launch-1.0 -q pipewiresrc path="$node" num-buffers=1 \
        ! videoconvert ! pngenc ! filesink location="$OUT/$1.png" >/dev/null 2>&1 \
        && [[ -s "$OUT/$1.png" ]]; then
        log "   frame: $1.png"
    else
        rm -f "$OUT/$1.png"
        log "   frame: no new frame"
    fi
}

snapshot() {
    step=$((step + 1))
    local name
    name=$(printf '%02d-%s' "$step" "$1")
    log "== $name"
    windows | sed 's/^/   window /' >> "$LOG"
    focus_info
    frame "$name"
}

base_layer() {
    xprop -root -f GAMESCOPECTRL_BASELAYER_APPID 32c -set GAMESCOPECTRL_BASELAYER_APPID "$1"
    log "   set GAMESCOPECTRL_BASELAYER_APPID=$1"
}

# Stand-in windows. kind: main | game | overlay.
cat > "$OUT/window.py" <<'EOF'
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
    sw, sh = screen.get_width(), screen.get_height()
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
EOF

open_window() {
    GDK_BACKEND=x11 python3 -I "$OUT/window.py" "$@" >> "$OUT/windows.log" 2>&1 &
    echo $!
}

log "env: DISPLAY=${DISPLAY:-} WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-}"
main=$(open_window main "$MAIN_APPID")
sleep 2
base_layer "$MAIN_APPID"
snapshot main

game=$(open_window game "$GAME_APPID")
sleep 2
base_layer "$GAME_APPID"
sleep 1
snapshot game

for round in "${ROUNDS[@]}"; do
    IFS=: read -r name prop appid shape input <<< "$round"
    log "-- round: $name ($prop=1, STEAM_GAME=$appid, $shape, STEAM_INPUT_FOCUS=$input)"
    extra=("$prop=1")
    [[ "$input" == 1 ]] && extra+=("STEAM_INPUT_FOCUS=1")
    overlay=$(open_window overlay "$appid" "$shape" "${extra[@]}")
    sleep "$WAIT"
    snapshot "$name-open"
    kill "$overlay" 2>/dev/null
    wait "$overlay" 2>/dev/null
    log "   overlay closed"
    sleep 2
    snapshot "$name-closed"
done

kill "$game" 2>/dev/null
sleep 1
base_layer "$MAIN_APPID"
sleep 1
snapshot game-closed
kill "$main" 2>/dev/null
log "done"
