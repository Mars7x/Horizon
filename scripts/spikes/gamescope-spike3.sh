#!/usr/bin/env bash
# Spike 3 (docs/GAMESCOPE_SESSION.md): hidden Steam. Does Flatpak Steam
# started with -silent stay hidden in gamescope? Does it write
# GAMESCOPECTRL_BASELAYER_APPID itself and take the screen from the main app?
# Do steam:// launches reach it, from gamescope and through the OpenURI
# portal (Horizon's route)? Can Steam's main window be brought forward and
# hidden again?
#
# Runs on the host with gamescope layered and Flatpak Steam installed and
# signed in. Changes nothing in Horizon. Horizon is Wayland-only today, so an
# X11 "Horizon stand-in" window plays the main app. It is tagged
# STEAM_GAME=7690000 (MAIN_APPID), an arbitrary ID: 769 is Steam's own, and
# Steam's windows win on it (first nested run).
#
#   bash scripts/spikes/gamescope-spike3.sh
#   STEAM_APPID=70 bash scripts/spikes/gamescope-spike3.sh   # another game
#
# From a desktop gamescope runs nested; from a text console, embedded.
# Embedded, stay on that console until "Results:" (about 6 minutes):
# switching away pauses gamescope and the run stops partway.
#
# A game's window is waited for (up to WAIT_GAME seconds) and closed through
# X11 (WM_DELETE_WINDOW, then XKillClient) before the next launch.
#
# "frame: no new frame" means gamescope sent no frame while it was captured,
# which also happens when a shown window is static. focus_info is the
# record of what gamescope shows.
#
# Guide button: while the game is open, the log marks a 20-second "guide"
# window. Press the controller's Guide (Steam/Home) button once inside it
# and note what appears; the snapshot after it records what gamescope shows.
#
# Steam must not be running when this starts; the script shuts it down at
# the end. Launching the game adds a little playtime in Steam.
#
# Output: ~/horizon-spikes/spike3-<time>/.
set -uo pipefail

STEAM=com.valvesoftware.Steam
APPID=${STEAM_APPID:-1055540}   # A Short Hike
MAIN_APPID=${MAIN_APPID:-7690000}
STEAM_APPID_SELF=769
WAIT_STEAM=${WAIT_STEAM:-90}
WAIT_GAME=${WAIT_GAME:-90}
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

if [[ "${1:-}" != --inner ]]; then
    for tool in gamescope gamescopectl xprop python3 flatpak gst-launch-1.0 gdbus; do
        command -v "$tool" >/dev/null || { echo "Missing: $tool" >&2; exit 1; }
    done
    flatpak info "$STEAM" >/dev/null 2>&1 || { echo "Flatpak Steam isn't installed." >&2; exit 1; }
    if flatpak ps --columns=application | grep -qx "$STEAM"; then
        echo "Steam is running. Quit it first." >&2
        exit 1
    fi

    OUT="$HOME/horizon-spikes/spike3-$(date +%Y%m%d-%H%M%S)"
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
        echo "steam: $(flatpak info "$STEAM" | grep -E 'Version|Commit' | tr -s ' ' | paste -sd' ')"
        echo "game: $APPID"
        echo "command: gamescope ${args[*]} -- $0 --inner"
    } > "$OUT/steps.log"

    gamescope "${args[@]}" -- "$0" --inner "$OUT" > "$OUT/gamescope.log" 2>&1
    echo "gamescope exit: $?" >> "$OUT/steps.log"
    # Never leave Steam running in the background after a failed run.
    if flatpak ps --columns=application | grep -qx "$STEAM"; then
        flatpak kill "$STEAM" 2>/dev/null
        echo "Steam was still running after gamescope; killed." >> "$OUT/steps.log"
    fi
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

root_props() {
    xprop -root 2>&1 | grep -E '^(GAMESCOPECTRL|STEAM_|GAMESCOPE_FOCUSABLE_APPS)' | sed 's/^/   root /'
}

focus_info() {
    local before
    before=$(wc -l < "$GSLOG")
    WAYLAND_DISPLAY="$GAMESCOPE_WAYLAND_DISPLAY" gamescopectl focus_info >/dev/null 2>&1
    sleep 1
    tail -n +"$((before + 1))" "$GSLOG" | sed 's/\x1b\[[0-9;]*m//g' \
        | grep -E 'focus: +(Global focus|Global overlay|Global input focus|  Focus window)' \
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
    root_props >> "$LOG"
    focus_info
    frame "$name"
}

base_layer() {
    xprop -root -f GAMESCOPECTRL_BASELAYER_APPID 32c -set GAMESCOPECTRL_BASELAYER_APPID "$1"
    log "   set GAMESCOPECTRL_BASELAYER_APPID=$1"
}

steam() { flatpak run "$STEAM" "$@" >> "$OUT/steam.log" 2>&1 & }

game_windows() { windows | awk -v t="steam_game=$APPID" '$4 == t {print $1}'; }

# Waits for the game's window (tagged by Steam) and logs how long it took.
wait_game() {
    local start=$SECONDS
    while (( SECONDS - start < WAIT_GAME )); do
        if [[ -n "$(game_windows)" ]]; then
            log "   game window after $((SECONDS - start)) s: $(game_windows | paste -sd' ')"
            return 0
        fi
        sleep 2
    done
    log "   no game window after $WAIT_GAME s"
    pgrep -af 'AppId=|SteamLaunch' 2>/dev/null | cut -c1-200 | sed 's/^/   process /' >> "$LOG"
    return 1
}

# Closes the game like a quit: WM_DELETE_WINDOW, then XKillClient.
close_game() {
    local wins
    wins=$(game_windows)
    [[ -z "$wins" ]] && { log "   no game window to close"; return; }
    python3 -I "$HERE/xwin.py" close $wins
    for _ in 1 2 3 4 5; do sleep 2; [[ -z "$(game_windows)" ]] && break; done
    if [[ -n "$(game_windows)" ]]; then
        python3 -I "$HERE/xwin.py" kill $(game_windows)
        sleep 3
        log "   game ignored WM_DELETE_WINDOW; XKillClient: $([[ -z "$(game_windows)" ]] && echo closed || echo 'still open')"
    else
        log "   game closed by WM_DELETE_WINDOW"
    fi
    sleep 3
}

game_round() {
    local route=$1
    wait_game
    snapshot "game-$route"
    if xprop -root GAMESCOPECTRL_BASELAYER_APPID | grep -q "= $APPID$"; then
        log "   Steam moved the base layer to the game"
    else
        log "   base layer not moved by Steam; setting it"
        base_layer "$APPID"
        sleep 2
        snapshot "game-$route-base-layer"
    fi
    if [[ "$route" == direct ]]; then
        log "-- guide window: press Guide now (20 s)"
        sleep 20
        snapshot guide
    fi
    close_game
    snapshot "game-$route-closed"
    base_layer "$MAIN_APPID"
    sleep 2
    snapshot "after-$route"
}

cat > "$OUT/stand_in.py" <<'EOF'
import subprocess, sys
import gi
gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
from gi.repository import Gtk, Gdk
css = Gtk.CssProvider()
css.load_from_data(b"window { background: #23395d; } label { font-size: 64px; color: white; }")
Gtk.StyleContext.add_provider_for_screen(Gdk.Screen.get_default(), css,
                                         Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION)
win = Gtk.Window(title="Horizon stand-in")
win.set_default_size(1280, 720)
win.add(Gtk.Label(label=f"Horizon stand-in ({sys.argv[1]})"))
win.connect("destroy", Gtk.main_quit)
win.show_all()
xid = win.get_window().get_xid()
subprocess.run(["xprop", "-id", str(xid), "-f", "STEAM_GAME", "32c", "-set", "STEAM_GAME", sys.argv[1]])
print(f"stand-in 0x{xid:x} STEAM_GAME={sys.argv[1]}", flush=True)
Gtk.main()
EOF

log "env: DISPLAY=${DISPLAY:-} WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-}"
snapshot empty

GDK_BACKEND=x11 python3 -I "$OUT/stand_in.py" "$MAIN_APPID" >> "$OUT/stand-in.log" 2>&1 &
stand_in=$!
sleep 3
base_layer "$MAIN_APPID"
sleep 1
snapshot stand-in

log "-- start Steam -silent"
steam -silent
for t in 20 45 "$WAIT_STEAM"; do
    sleep $((t - ${prev:-0}))
    prev=$t
    snapshot "steam-${t}s"
done
grep -qx "$STEAM" <(flatpak ps --columns=application) \
    && log "   Steam running" || log "   Steam not running; see steam.log"

log "-- game $APPID via steam:// from gamescope"
steam "steam://rungameid/$APPID"
game_round direct

log "-- game $APPID via the OpenURI portal (Horizon's route)"
gdbus call --session --dest org.freedesktop.portal.Desktop \
    --object-path /org/freedesktop/portal/desktop \
    --method org.freedesktop.portal.OpenURI.OpenURI "" "steam://rungameid/$APPID" "{}" \
    >> "$LOG" 2>&1
game_round portal

log "-- Steam main window: open, bring forward, hide"
steam steam://open/main
sleep 15
snapshot steam-open
base_layer "$STEAM_APPID_SELF"
sleep 2
snapshot steam-forward
base_layer "$MAIN_APPID"
sleep 2
snapshot steam-hidden

log "-- shut down Steam"
steam -shutdown
sleep 25
snapshot steam-shutdown
kill "$stand_in" 2>/dev/null
log "done"
