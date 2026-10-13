#!/usr/bin/env bash
# Decision 17 check (docs/GAMESCOPE_SESSION.md): if the session's host script
# puts gamescope's DISPLAY into the user's systemd environment, do Flatpak
# games launched through Horizon's route (flatpak-spawn --host, as
# platform::flatpak_host does) open on gamescope's display?
#
# Runs on the host with gamescope layered, the Horizon Flatpak and Azahar
# (org.azahar_emu.Azahar) installed. Changes nothing in Horizon.
#
# It must run with no desktop behind it: log out of GNOME first, log in on a
# text console (Ctrl+Alt+F3), then:
#
#   bash ~/Projects/Horizon/scripts/spikes/gamescope-spike-display.sh
#
# Stay on that console until "Results:" (about 2 minutes).
#
# Rounds:
#   as-found  set DISPLAY and XDG_CURRENT_DESKTOP=gamescope in the systemd
#             environment, then launch (the helper starts now if it isn't
#             running, or keeps whatever environment it had)
#   restart   restart flatpak-session-helper, then launch again
# Each launch: the environment a host command gets, whether Azahar's window
# appears in gamescope, and (tagged 0x7F000001, base layer set) a frame.
# At the end it restores the systemd environment and stops the helper.
#
# Output: ~/horizon-spikes/spike-display-<time>/.
set -uo pipefail

HORIZON=io.github.Mars7x.Horizon
GAME=org.azahar_emu.Azahar
TAG=$((0x7F000001))
HELPER=flatpak-session-helper.service
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

if [[ "${1:-}" != --inner ]]; then
    for tool in gamescope gamescopectl xprop python3 flatpak gst-launch-1.0 systemctl; do
        command -v "$tool" >/dev/null || { echo "Missing: $tool" >&2; exit 1; }
    done
    for app in "$HORIZON" "$GAME"; do
        flatpak info "$app" >/dev/null 2>&1 || { echo "Not installed: $app" >&2; exit 1; }
    done
    if pgrep -u "$USER" -x gnome-shell >/dev/null || [[ -n "${WAYLAND_DISPLAY:-}${DISPLAY:-}" ]]; then
        echo "A desktop is running. Log out of GNOME, log in on a text console, and run this there." >&2
        exit 1
    fi

    OUT="$HOME/horizon-spikes/spike-display-$(date +%Y%m%d-%H%M%S)"
    mkdir -p "$OUT"
    systemctl --user show-environment > "$OUT/systemd-env.before"
    {
        echo "mode: embedded"
        echo "gamescope: $(rpm -q gamescope 2>/dev/null || echo unknown)"
        echo "helper before gamescope: $(systemctl --user is-active "$HELPER")"
    } > "$OUT/steps.log"

    gamescope -e --backend drm -- "$0" --inner "$OUT" > "$OUT/gamescope.log" 2>&1
    echo "gamescope exit: $?" >> "$OUT/steps.log"

    flatpak kill "$GAME" 2>/dev/null
    for var in DISPLAY XDG_CURRENT_DESKTOP WAYLAND_DISPLAY; do
        old=$(grep "^$var=" "$OUT/systemd-env.before" | head -1)
        if [[ -n "$old" ]]; then
            systemctl --user set-environment "$old"
        else
            systemctl --user unset-environment "$var"
        fi
    done
    systemctl --user stop "$HELPER" 2>/dev/null
    echo "restored systemd environment, stopped the helper" >> "$OUT/steps.log"
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
        | grep -E 'focus: +Global focus' | sed 's/.*focus: */   /' >> "$LOG"
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

host_env() {
    flatpak run --command=flatpak-spawn "$HORIZON" --host \
        sh -c 'echo DISPLAY=$DISPLAY WAYLAND_DISPLAY=$WAYLAND_DISPLAY XDG_CURRENT_DESKTOP=$XDG_CURRENT_DESKTOP' \
        2>&1 | tail -1
}

launch_round() {
    local name=$1 before new
    log "-- round: $name"
    log "   helper: $(systemctl --user is-active "$HELPER"), since $(systemctl --user show "$HELPER" -p ActiveEnterTimestamp --value)"
    log "   host command env: $(host_env)"
    before=$(windows | awk '{print $1}')
    # The way Horizon's flatpak_host runs a Flatpak game today.
    flatpak run --command=flatpak-spawn "$HORIZON" --host flatpak run "$GAME" >> "$OUT/apps.log" 2>&1 &
    sleep 15
    flatpak ps --columns=application | grep -qx "$GAME" && log "   $GAME running" || log "   $GAME not running"
    new=$(comm -13 <(echo "$before" | sort) <(windows | awk '{print $1}' | sort) | grep '^0x')
    if [[ -n "$new" ]]; then
        log "   new window in gamescope: $(echo $new)"
        python3 -I "$HERE/xwin.py" tag "$TAG" $new
        xprop -root -f GAMESCOPECTRL_BASELAYER_APPID 32c -set GAMESCOPECTRL_BASELAYER_APPID "$TAG"
        sleep 2
    else
        log "   no new window in gamescope"
    fi
    snapshot "$name"
    flatpak kill "$GAME" 2>/dev/null
    sleep 3
}

log "env: DISPLAY=$DISPLAY"
# No host command before this: it would start the helper with the old
# environment, which a session's host script avoids.
log "helper at start: $(systemctl --user is-active "$HELPER")"
systemctl --user set-environment "DISPLAY=$DISPLAY" XDG_CURRENT_DESKTOP=gamescope
systemctl --user unset-environment WAYLAND_DISPLAY
log "set systemd env: $(systemctl --user show-environment \
    | grep -E '^(DISPLAY|XDG_CURRENT_DESKTOP|WAYLAND_DISPLAY)=' | paste -sd' ')"

launch_round as-found

systemctl --user restart "$HELPER"
log "restarted $HELPER"
sleep 1
launch_round restart

log "done"
