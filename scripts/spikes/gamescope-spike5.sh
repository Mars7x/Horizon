#!/usr/bin/env bash
# Spike 5 (docs/GAMESCOPE_SESSION.md): screenshots. Gamescope takes one on
# request; which request works, where does the file land, in what format,
# and does it include an overlay (the quick menu is open when its Screenshot
# button is pressed)?
#
# Runs on the host with gamescope layered. Changes nothing in Horizon. X11
# stand-ins (scripts/spikes/standin.py) play the main app, an animated game
# and the quick-menu overlay (full-screen, STEAM_OVERLAY=1,
# STEAM_INPUT_FOCUS=1, as found in the overlay spike).
#
#   bash scripts/spikes/gamescope-spike5.sh
#
# From a desktop gamescope runs nested; from a text console, embedded.
# Embedded, stay on that console until "Results:" (about 2 minutes).
#
# Requests, each with the game shown, then again with the overlay up:
#   ctl-path      gamescopectl screenshot <path in the results folder>
#   ctl-default   gamescopectl screenshot (no path)
#   prop          root GAMESCOPECTRL_REQUEST_SCREENSHOT=1 (Steam's way)
#   prop-debug    root GAMESCOPECTRL_DEBUG_REQUEST_SCREENSHOT=1
# New files are looked for in the results folder, /tmp, ~/Pictures and
# $XDG_RUNTIME_DIR, and copied into the results folder.
#
# Output: ~/horizon-spikes/spike5-<time>/.
set -uo pipefail

MAIN_APPID=7690000
GAME_APPID=4242
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

if [[ "${1:-}" != --inner ]]; then
    for tool in gamescope gamescopectl xprop python3 file gst-launch-1.0; do
        command -v "$tool" >/dev/null || { echo "Missing: $tool" >&2; exit 1; }
    done
    OUT="$HOME/horizon-spikes/spike5-$(date +%Y%m%d-%H%M%S)"
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
SEARCH=("$OUT" /tmp "$HOME/Pictures" "${XDG_RUNTIME_DIR:-/run/user/$UID}")
STAMP="$OUT/.stamp"

log() { echo "[$(date +%T)] $*" >> "$LOG"; }

open_window() {
    GDK_BACKEND=x11 python3 -I "$HERE/standin.py" "$@" >> "$OUT/windows.log" 2>&1 &
    echo $!
}

base_layer() {
    xprop -root -f GAMESCOPECTRL_BASELAYER_APPID 32c -set GAMESCOPECTRL_BASELAYER_APPID "$1"
}

ctl() { WAYLAND_DISPLAY="$GAMESCOPE_WAYLAND_DISPLAY" gamescopectl "$@" 2>&1; }

# What gamescope shows: its focus state and one frame of its PipeWire stream.
shown() {
    local lines node
    lines=$(wc -l < "$GSLOG")
    ctl focus_info >/dev/null
    sleep 1
    tail -n +"$((lines + 1))" "$GSLOG" | sed 's/\x1b\[[0-9;]*m//g' \
        | grep -E 'focus: +(Global focus|Global input focus|Global overlay)' \
        | sed 's/.*focus: */   /' >> "$LOG"
    node=$(sed -n 's/.*stream available on node ID: \([0-9]*\).*/\1/p' "$GSLOG" | tail -1)
    if [[ -n "$node" ]] && timeout 10 gst-launch-1.0 -q pipewiresrc path="$node" num-buffers=1 \
        ! videoconvert ! pngenc ! filesink location="$OUT/screen-$1.png" >/dev/null 2>&1; then
        log "   on screen: screen-$1.png"
    fi
}

# Files newer than the stamp, in the places a screenshot might land.
new_files() {
    find "${SEARCH[@]}" -maxdepth 1 -type f -newer "$STAMP" \
        \( -iname '*.png' -o -iname '*.avif' -o -iname '*.jxl' -o -iname '*gamescope*' \) \
        ! -name '*.log' 2>/dev/null
}

request() {
    local name=$1 lines found f
    shift
    log "-- $name: $*"
    touch "$STAMP"
    sleep 1
    lines=$(wc -l < "$GSLOG")
    case "$name" in
        *ctl-path) ctl screenshot "$OUT/$name.png" | sed 's/^/   ctl /' >> "$LOG" ;;
        *ctl-default) ctl screenshot | sed 's/^/   ctl /' >> "$LOG" ;;
        *prop) xprop -root -f GAMESCOPECTRL_REQUEST_SCREENSHOT 32c \
                   -set GAMESCOPECTRL_REQUEST_SCREENSHOT 1 ;;
        *prop-debug) xprop -root -f GAMESCOPECTRL_DEBUG_REQUEST_SCREENSHOT 32c \
                   -set GAMESCOPECTRL_DEBUG_REQUEST_SCREENSHOT 1 ;;
    esac
    sleep 4
    tail -n +"$((lines + 1))" "$GSLOG" | sed 's/\x1b\[[0-9;]*m//g' | grep -i 'screenshot' \
        | sed 's/^/   gamescope /' >> "$LOG"
    found=$(new_files)
    if [[ -z "$found" ]]; then
        log "   no file"
        return
    fi
    while read -r f; do
        log "   file: $f ($(stat -c %s "$f") bytes, $(file -b "$f" | cut -c1-60))"
        [[ "$f" == "$OUT/"* ]] || cp "$f" "$OUT/$name-$(basename "$f")"
    done <<< "$found"
}

log "env: DISPLAY=${DISPLAY:-} GAMESCOPE_WAYLAND_DISPLAY=${GAMESCOPE_WAYLAND_DISPLAY:-}"
log "gamescopectl help screenshot: $(ctl help 2>/dev/null | grep -i screenshot | paste -sd' ')"
main=$(open_window main "$MAIN_APPID")
sleep 2
game=$(open_window game "$GAME_APPID")
sleep 2
base_layer "$GAME_APPID"
sleep 2
log "game shown"
shown game

for r in ctl-path ctl-default prop prop-debug; do
    request "$r"
done

overlay=$(open_window overlay "$MAIN_APPID" full STEAM_OVERLAY=1 STEAM_INPUT_FOCUS=1)
sleep 3
log "overlay up"
shown overlay
for r in ctl-path ctl-default prop prop-debug; do
    request "overlay-$r"
done
kill "$overlay" "$game" "$main" 2>/dev/null
log "done"
