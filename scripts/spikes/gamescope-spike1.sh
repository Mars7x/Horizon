#!/usr/bin/env bash
# Spike 1 (docs/GAMESCOPE_SESSION.md): can a Wayland Horizon be gamescope's
# main app, give way to a window on top, and come back when it closes?
#
# Runs on the host (not in a toolbox), with gamescope layered on the image and
# the Horizon Flatpak installed. Changes nothing in Horizon.
#
#   bash scripts/spikes/gamescope-spike1.sh [steam|plain]
#
# From a desktop gamescope runs nested; from a text console, embedded.
# "steam" (default) starts gamescope the way gamescope-session-plus does (-e,
# Steam mode); "plain" leaves -e out. Both add --expose-wayland.
#
# At each step the log records gamescope's focus state (gamescopectl
# focus_info) and saves one frame of gamescope's output, taken from its
# PipeWire stream (gamescopectl screenshot saved nothing on 3.16.29).
#
# Rounds: a Wayland test window, an X11 test window, and an X11 test window
# tagged STEAM_GAME=4242 the way Steam tags a game. In Steam mode that round
# also sets GAMESCOPECTRL_BASELAYER_APPID=4242 while the window is open, as
# Steam would.
#
# Output: ~/horizon-spikes/spike1-<mode>-<time>/ (steps.log, gamescope.log,
# *.png).
set -uo pipefail

APP_ID=io.github.Mars7x.Horizon
WAIT_HORIZON=${WAIT_HORIZON:-15}
WAIT_WINDOW=${WAIT_WINDOW:-5}

if [[ "${1:-}" != --inner ]]; then
    MODE=${1:-steam}
    [[ "$MODE" == steam || "$MODE" == plain ]] || { echo "Mode: steam or plain" >&2; exit 1; }
    for tool in gamescope gamescopectl xprop python3 flatpak gst-launch-1.0; do
        command -v "$tool" >/dev/null || { echo "Missing: $tool" >&2; exit 1; }
    done
    flatpak info "$APP_ID" >/dev/null 2>&1 || { echo "Horizon Flatpak isn't installed." >&2; exit 1; }
    if flatpak ps --columns=application | grep -qx "$APP_ID"; then
        echo "Horizon is already running. Close it first." >&2
        exit 1
    fi

    OUT="$HOME/horizon-spikes/spike1-$MODE-$(date +%Y%m%d-%H%M%S)"
    mkdir -p "$OUT"

    args=(--expose-wayland)
    [[ "$MODE" == steam ]] && args+=(-e)
    if [[ -n "${WAYLAND_DISPLAY:-}${DISPLAY:-}" ]]; then
        mode=nested
        args+=(-W 1920 -H 1080 -w 1920 -h 1080)
    else
        mode=embedded
        args+=(--backend drm)
    fi
    {
        echo "mode: $mode, $MODE"
        echo "gamescope: $(rpm -q gamescope 2>/dev/null || gamescope --version 2>&1 | head -1)"
        echo "horizon: $(flatpak info "$APP_ID" | grep -E 'Version|Commit' | tr -s ' ' | paste -sd' ')"
        echo "command: gamescope ${args[*]} -- $0 --inner"
    } > "$OUT/steps.log"

    gamescope "${args[@]}" -- "$0" --inner "$OUT" "$MODE" > "$OUT/gamescope.log" 2>&1
    echo "gamescope exit: $?" >> "$OUT/steps.log"
    echo "Results: $OUT"
    exit 0
fi

# ---- Inside gamescope ------------------------------------------------------
OUT=$2
MODE=$3
LOG="$OUT/steps.log"
GSLOG="$OUT/gamescope.log"
step=0

log() { echo "[$(date +%T)] $*" >> "$LOG"; }

# Gamescope prints focus_info into its own log; copy the new lines across.
focus_info() {
    local before
    before=$(wc -l < "$GSLOG")
    WAYLAND_DISPLAY="$GAMESCOPE_WAYLAND_DISPLAY" gamescopectl focus_info >/dev/null 2>&1
    sleep 1
    tail -n +"$((before + 1))" "$GSLOG" | sed 's/\x1b\[[0-9;]*m//g' \
        | grep -E 'focus: +(Global focus|Global overlay|  Connector|  Focus window)' \
        | sed 's/.*focus: */   /' >> "$LOG"
}

# One frame of gamescope's output. No frame means gamescope showed nothing.
frame() {
    local node
    node=$(sed -n 's/.*stream available on node ID: \([0-9]*\).*/\1/p' "$GSLOG" | tail -1)
    if [[ -n "$node" ]] && timeout 10 gst-launch-1.0 -q pipewiresrc path="$node" num-buffers=1 \
        ! videoconvert ! pngenc ! filesink location="$OUT/$1.png" >/dev/null 2>&1 \
        && [[ -s "$OUT/$1.png" ]]; then
        log "   frame: $1.png"
    else
        rm -f "$OUT/$1.png"
        log "   frame: none (nothing on screen)"
    fi
}

snapshot() {
    step=$((step + 1))
    local name
    name=$(printf '%02d-%s' "$step" "$1")
    log "== $name"
    focus_info
    frame "$name"
}

# A large, plainly coloured window so the screenshot shows it at a glance.
cat > "$OUT/test_window.py" <<'EOF'
import os, subprocess, sys
import gi
toolkit, label, tag = sys.argv[1], sys.argv[2], sys.argv[3]
if toolkit == "gtk4":
    gi.require_version("Gtk", "4.0")
    gi.require_version("Gdk", "4.0")
    from gi.repository import Gtk, Gdk
    app = Gtk.Application(application_id="io.github.Mars7x.HorizonSpikeTest")
    def on_activate(app):
        css = Gtk.CssProvider()
        css.load_from_string("window { background: #d0208a; } label { font-size: 64px; color: white; }")
        Gtk.StyleContext.add_provider_for_display(Gdk.Display.get_default(), css,
                                                  Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION)
        win = Gtk.ApplicationWindow(application=app, title=label)
        win.set_default_size(1280, 720)
        win.set_child(Gtk.Label(label=label))
        win.present()
    app.connect("activate", on_activate)
    app.run([])
else:
    gi.require_version("Gtk", "3.0")
    gi.require_version("Gdk", "3.0")
    from gi.repository import Gtk, Gdk, GLib
    css = Gtk.CssProvider()
    css.load_from_data(b"window { background: #1f8a3a; } label { font-size: 64px; color: white; }")
    Gtk.StyleContext.add_provider_for_screen(Gdk.Screen.get_default(), css,
                                             Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION)
    win = Gtk.Window(title=label)
    win.set_default_size(1280, 720)
    win.add(Gtk.Label(label=label))
    win.connect("destroy", Gtk.main_quit)
    win.show_all()
    if tag != "none":
        xid = win.get_window().get_xid()
        subprocess.run(["xprop", "-id", str(xid), "-f", "STEAM_GAME", "32c",
                        "-set", "STEAM_GAME", tag], check=False)
        print(f"STEAM_GAME={tag} on 0x{xid:x}", flush=True)
    Gtk.main()
EOF

test_round() {
    local name=$1 toolkit=$2 backend=$3 tag=$4 pid
    log "-- round: $name"
    if [[ "$MODE" == steam && "$tag" != none ]]; then
        xprop -root -f GAMESCOPECTRL_BASELAYER_APPID 32c -set GAMESCOPECTRL_BASELAYER_APPID "$tag"
        log "   GAMESCOPECTRL_BASELAYER_APPID=$tag"
    fi
    GDK_BACKEND=$backend python3 -I "$OUT/test_window.py" "$toolkit" "Test window: $name" "$tag" \
        >> "$OUT/test-window.log" 2>&1 &
    pid=$!
    sleep "$WAIT_WINDOW"
    if kill -0 "$pid" 2>/dev/null; then
        log "   test window running (pid $pid)"
    else
        log "   test window exited early; see test-window.log"
    fi
    snapshot "$name-open"
    kill "$pid" 2>/dev/null
    wait "$pid" 2>/dev/null
    log "   test window closed"
    if [[ "$MODE" == steam && "$tag" != none ]]; then
        xprop -root -remove GAMESCOPECTRL_BASELAYER_APPID
        log "   GAMESCOPECTRL_BASELAYER_APPID removed"
    fi
    sleep "$WAIT_WINDOW"
    snapshot "$name-closed"
}

log "env: DISPLAY=${DISPLAY:-} WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-} GAMESCOPE_WAYLAND_DISPLAY=${GAMESCOPE_WAYLAND_DISPLAY:-}"
snapshot empty

flatpak run "$APP_ID" > "$OUT/horizon.log" 2>&1 &
log "started Horizon"
sleep "$WAIT_HORIZON"
flatpak ps --columns=application | grep -qx "$APP_ID" \
    && log "   Horizon running" || log "   Horizon not running; see horizon.log"
snapshot horizon

test_round wayland gtk4 wayland none
test_round x11 gtk3 x11 none
test_round x11-steam-game gtk3 x11 4242

flatpak kill "$APP_ID" 2>/dev/null
log "closed Horizon"
sleep 3
snapshot horizon-closed
log "done"
