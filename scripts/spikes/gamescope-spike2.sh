#!/usr/bin/env bash
# Spike 2 (docs/GAMESCOPE_SESSION.md): non-Steam windows. Does gamescope show
# a Flatpak or Heroic window with no STEAM_GAME? Does it once another X11
# client tags it, as Horizon would? Which display do games get?
#
# Runs on the host with gamescope layered and the apps below installed.
# Changes nothing in Horizon, and Horizon isn't shown: it is Wayland-only
# today, so this script sets the base layer itself.
#
#   bash scripts/spikes/gamescope-spike2.sh [session|expose]
#
# From a desktop gamescope runs nested; from a text console, embedded.
# Embedded, stay on that console until "Results:": switching away pauses
# gamescope and the run stops partway.
#
# "session" (default) starts gamescope as the session plans to: -e and no
# --expose-wayland, so apps see no Wayland display. "expose" adds
# --expose-wayland, to see which apps then pick Wayland and vanish.
#
# Each round starts an app, records gamescope's focus and one frame, lists
# the new top-level X11 windows, tags them STEAM_GAME=<id> and sets
# GAMESCOPECTRL_BASELAYER_APPID=<id>, records again, then closes the app.
# The "horizon-route" round starts the app through Horizon's own sandbox
# (flatpak-spawn --host, as platform::flatpak_host does) to see where it
# lands.
#
# HEROIC_GAME=<appName> also launches that Heroic game (gui=false), as
# Horizon would. Off by default: it may need a sign-in or an update.
#
# Output: ~/horizon-spikes/spike2-<mode>-<time>/.
set -uo pipefail

HORIZON=io.github.Mars7x.Horizon
WAIT_APP=${WAIT_APP:-15}
ROUNDS=(
    "dolphin:org.DolphinEmu.dolphin-emu:5001:direct"
    "azahar:org.azahar_emu.Azahar:5002:direct"
    "highscore:app.drey.Highscore.Devel:5003:direct"
    "heroic:com.heroicgameslauncher.hgl:5004:direct"
    "horizon-route:org.azahar_emu.Azahar:5005:horizon"
)

if [[ "${1:-}" != --inner ]]; then
    MODE=${1:-session}
    [[ "$MODE" == session || "$MODE" == expose ]] || { echo "Mode: session or expose" >&2; exit 1; }
    for tool in gamescope gamescopectl xprop python3 flatpak gst-launch-1.0; do
        command -v "$tool" >/dev/null || { echo "Missing: $tool" >&2; exit 1; }
    done
    for round in "${ROUNDS[@]}"; do
        IFS=: read -r _ app _ _ <<< "$round"
        flatpak info "$app" >/dev/null 2>&1 || { echo "Not installed: $app" >&2; exit 1; }
        if flatpak ps --columns=application | grep -qx "$app"; then
            echo "Already running: $app. Close it first." >&2
            exit 1
        fi
    done

    OUT="$HOME/horizon-spikes/spike2-$MODE-$(date +%Y%m%d-%H%M%S)"
    mkdir -p "$OUT"

    args=(-e)
    [[ "$MODE" == expose ]] && args+=(--expose-wayland)
    if [[ -n "${WAYLAND_DISPLAY:-}${DISPLAY:-}" ]]; then
        mode=nested
        args+=(-W 1920 -H 1080 -w 1920 -h 1080)
    else
        mode=embedded
        args+=(--backend drm)
    fi
    {
        echo "mode: $mode, $MODE"
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

# Lists mapped top-level X11 windows, or tags given windows with STEAM_GAME.
cat > "$OUT/xwin.py" <<'EOF'
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
EOF

windows() { python3 -I "$OUT/xwin.py" list 2>&1; }

focus_info() {
    local before
    before=$(wc -l < "$GSLOG")
    WAYLAND_DISPLAY="$GAMESCOPE_WAYLAND_DISPLAY" gamescopectl focus_info >/dev/null 2>&1
    sleep 1
    tail -n +"$((before + 1))" "$GSLOG" | sed 's/\x1b\[[0-9;]*m//g' \
        | grep -E 'focus: +(Global focus|  Focus window)' \
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
        log "   frame: none (nothing on screen)"
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

launch() {
    local app=$1 route=$2
    if [[ "$route" == horizon ]]; then
        # The way Horizon's flatpak_host runs a Flatpak game today.
        flatpak run --command=flatpak-spawn "$HORIZON" --host flatpak run "$app" \
            >> "$OUT/apps.log" 2>&1 &
    else
        flatpak run "$app" >> "$OUT/apps.log" 2>&1 &
    fi
}

log "env: DISPLAY=${DISPLAY:-} WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-} GAMESCOPE_WAYLAND_DISPLAY=${GAMESCOPE_WAYLAND_DISPLAY:-}"
log "host command env via Horizon's sandbox: $(flatpak run --command=flatpak-spawn "$HORIZON" --host \
    sh -c 'echo DISPLAY=$DISPLAY WAYLAND_DISPLAY=$WAYLAND_DISPLAY' 2>/dev/null | tail -1)"
snapshot empty

for round in "${ROUNDS[@]}"; do
    IFS=: read -r name app id route <<< "$round"
    log "-- round: $name ($app, $route)"
    before=$(windows | awk '{print $1}')
    launch "$app" "$route"
    sleep "$WAIT_APP"
    if [[ "$name" == heroic && -n "${HEROIC_GAME:-}" ]]; then
        flatpak run com.heroicgameslauncher.hgl \
            "heroic://launch?appName=$HEROIC_GAME&runner=legendary&gui=false" >> "$OUT/apps.log" 2>&1 &
        log "   launched Heroic game $HEROIC_GAME"
        sleep "$WAIT_APP"
    fi
    flatpak ps --columns=application | grep -qx "$app" \
        && log "   $app running" || log "   $app not running; see apps.log"
    snapshot "$name-untagged"

    new=$(comm -13 <(echo "$before" | sort) <(windows | awk '{print $1}' | sort) | grep '^0x')
    if [[ -n "$new" ]]; then
        python3 -I "$OUT/xwin.py" tag "$id" $new
        log "   tagged STEAM_GAME=$id on: $(echo $new)"
    else
        log "   no new X11 window to tag"
    fi
    xprop -root -f GAMESCOPECTRL_BASELAYER_APPID 32c -set GAMESCOPECTRL_BASELAYER_APPID "$id"
    log "   GAMESCOPECTRL_BASELAYER_APPID=$id"
    sleep 2
    snapshot "$name-tagged"

    flatpak kill "$app" 2>/dev/null
    log "   closed $app"
    xprop -root -remove GAMESCOPECTRL_BASELAYER_APPID
    sleep 5
    snapshot "$name-closed"
done
log "done"
