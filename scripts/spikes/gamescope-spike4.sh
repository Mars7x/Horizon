#!/usr/bin/env bash
# Spike 4 (docs/GAMESCOPE_SESSION.md): portals in a bare gamescope session.
# Which of Settings, FileChooser and OpenURI answer Horizon, and do their
# windows reach the screen?
#
# Runs on the host with gamescope layered and the Horizon Flatpak installed.
# Changes nothing in Horizon. Portal calls are made from inside Horizon's
# sandbox (flatpak run --command=gdbus), as Horizon would make them.
#
# It must run with no desktop behind it: log out of GNOME first, log in on a
# text console (Ctrl+Alt+F3), then:
#
#   bash ~/Projects/Horizon/scripts/spikes/gamescope-spike4.sh
#
# Stay on that console until "Results:" (about 3 minutes).
#
# Like gamescope-session-plus, it sets XDG_CURRENT_DESKTOP=gamescope and
# DISPLAY in the user's systemd environment, so portals start inside
# gamescope, and starts graphical-session.target through a temporary session
# target (~/.config/systemd/user/horizon-spike4-session.target): the portal
# service refuses to start without it (Requisite=graphical-session.target),
# as the first run showed. Two rounds:
#   as-is  no portal configuration for gamescope (what Fedora picks)
#   gtk    ~/.config/xdg-desktop-portal/gamescope-portals.conf: default=gtk
# At the end it stops and removes the session target, removes that file,
# restores the systemd environment and stops the portals so the next login
# starts them fresh.
#
# Portal windows are listed when they appear, then tagged with the main
# app's ID (7690000) and shown with the base layer, then closed. OpenURI
# opens https://flathub.org in the default browser.
#
# Output: ~/horizon-spikes/spike4-<time>/.
set -uo pipefail

HORIZON=io.github.Mars7x.Horizon
MAIN_APPID=7690000
URL=https://flathub.org
CONF_DIR="$HOME/.config/xdg-desktop-portal"
CONF="$CONF_DIR/gamescope-portals.conf"
TARGET=horizon-spike4-session.target
TARGET_FILE="$HOME/.config/systemd/user/$TARGET"
PORTALS=(xdg-desktop-portal xdg-desktop-portal-gtk xdg-desktop-portal-gnome)
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

if [[ "${1:-}" != --inner ]]; then
    for tool in gamescope gamescopectl xprop python3 flatpak gst-launch-1.0 gdbus systemctl; do
        command -v "$tool" >/dev/null || { echo "Missing: $tool" >&2; exit 1; }
    done
    flatpak info "$HORIZON" >/dev/null 2>&1 || { echo "Horizon Flatpak isn't installed." >&2; exit 1; }
    if pgrep -u "$USER" -x gnome-shell >/dev/null || [[ -n "${WAYLAND_DISPLAY:-}${DISPLAY:-}" ]]; then
        echo "A desktop is running. Log out of GNOME, log in on a text console, and run this there." >&2
        exit 1
    fi
    for f in "$CONF" "$TARGET_FILE"; do
        if [[ -e "$f" ]]; then
            echo "$f already exists; not touching it." >&2
            exit 1
        fi
    done

    OUT="$HOME/horizon-spikes/spike4-$(date +%Y%m%d-%H%M%S)"
    mkdir -p "$OUT"
    systemctl --user show-environment > "$OUT/systemd-env.before"
    {
        echo "mode: embedded"
        echo "gamescope: $(rpm -q gamescope 2>/dev/null || echo unknown)"
        echo "portals: $(rpm -qa 'xdg-desktop-portal*' | sort | paste -sd' ')"
        echo "https handler: $(gio mime x-scheme-handler/https 2>/dev/null | head -1)"
    } > "$OUT/steps.log"

    gamescope -e --backend drm -- "$0" --inner "$OUT" > "$OUT/gamescope.log" 2>&1
    echo "gamescope exit: $?" >> "$OUT/steps.log"

    # Restore: session target, config file, systemd environment, portals.
    systemctl --user stop "$TARGET" 2>/dev/null
    rm -f "$TARGET_FILE"
    systemctl --user daemon-reload
    rm -f "$CONF"
    rmdir "$CONF_DIR" 2>/dev/null
    for var in DISPLAY XDG_CURRENT_DESKTOP WAYLAND_DISPLAY; do
        old=$(grep "^$var=" "$OUT/systemd-env.before" | head -1)
        if [[ -n "$old" ]]; then
            systemctl --user set-environment "$old"
        else
            systemctl --user unset-environment "$var"
        fi
    done
    systemctl --user stop "${PORTALS[@]}" 2>/dev/null
    echo "removed $TARGET, restored systemd environment, stopped portals" >> "$OUT/steps.log"
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
        | grep -E 'focus: +(Global focus|Global input focus)' | sed 's/.*focus: */   /' >> "$LOG"
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

# A portal call from inside Horizon's sandbox.
portal() {
    timeout 30 flatpak run --command=gdbus "$HORIZON" call --session \
        --dest org.freedesktop.portal.Desktop --object-path /org/freedesktop/portal/desktop \
        --method "$@" 2>&1 | sed 's/^/   reply /' >> "$LOG"
}

# Shows the windows a portal opened, then closes them.
show_and_close() {
    local before=$1 name=$2 new
    new=$(comm -13 <(echo "$before" | sort) <(windows | awk '{print $1}' | sort) | grep '^0x')
    snapshot "$name-untagged"
    if [[ -z "$new" ]]; then
        log "   no new window"
        return
    fi
    log "   new windows: $(echo $new)"
    python3 -I "$HERE/xwin.py" tag "$MAIN_APPID" $new
    xprop -root -f GAMESCOPECTRL_BASELAYER_APPID 32c -set GAMESCOPECTRL_BASELAYER_APPID "$MAIN_APPID"
    log "   tagged STEAM_GAME=$MAIN_APPID, base layer $MAIN_APPID"
    sleep 2
    snapshot "$name-tagged"
    python3 -I "$HERE/xwin.py" close $new
    sleep 3
    left=$(windows | awk '{print $1}' | grep -Fx -f <(echo "$new" | tr ' ' '\n') || true)
    [[ -n "$left" ]] && python3 -I "$HERE/xwin.py" kill $left
    log "   closed"
    sleep 2
}

run_round() {
    local round=$1
    log "-- round: $round"
    systemctl --user stop "${PORTALS[@]}" 2>/dev/null
    sleep 1

    log "   Settings: color-scheme"
    portal org.freedesktop.portal.Settings.ReadOne org.freedesktop.appearance color-scheme
    log "   Settings: clock-format"
    portal org.freedesktop.portal.Settings.ReadOne org.gnome.desktop.interface clock-format
    log "   Settings: accent-color"
    portal org.freedesktop.portal.Settings.ReadOne org.freedesktop.appearance accent-color

    # The Settings calls started the portal; watch its Response signals now.
    gdbus monitor --session --dest org.freedesktop.portal.Desktop > "$OUT/responses-$round.log" 2>&1 &
    local monitor=$!
    sleep 1

    log "   FileChooser: OpenFile"
    local before
    before=$(windows | awk '{print $1}')
    portal org.freedesktop.portal.FileChooser.OpenFile "" "Spike 4" "{}"
    sleep 8
    show_and_close "$before" "$round-filechooser"

    log "   OpenURI: $URL"
    before=$(windows | awk '{print $1}')
    portal org.freedesktop.portal.OpenURI.OpenURI "" "$URL" "{}"
    sleep 15
    show_and_close "$before" "$round-openuri"

    sleep 2
    kill "$monitor" 2>/dev/null
    grep -E 'Response|Error' "$OUT/responses-$round.log" | sed 's/^/   signal /' >> "$LOG"
    log "   running portal backends: $(systemctl --user list-units --state=running --no-legend \
        'xdg-desktop-portal*' 2>/dev/null | awk '{print $1}' | paste -sd' ')"
    journalctl --user --since "-2min" -u 'xdg-desktop-portal*' --no-pager 2>/dev/null \
        | tail -20 > "$OUT/portal-journal-$round.log"
}

systemctl --user set-environment "DISPLAY=$DISPLAY" XDG_CURRENT_DESKTOP=gamescope
systemctl --user unset-environment WAYLAND_DISPLAY
mkdir -p "$(dirname "$TARGET_FILE")"
cat > "$TARGET_FILE" <<'UNIT'
[Unit]
Description=Horizon spike 4 session (temporary)
BindsTo=graphical-session.target
Before=graphical-session.target
UNIT
systemctl --user daemon-reload
systemctl --user start "$TARGET" >> "$LOG" 2>&1
log "graphical-session.target: $(systemctl --user is-active graphical-session.target)"
log "env: DISPLAY=$DISPLAY; systemd user env now: $(systemctl --user show-environment \
    | grep -E '^(DISPLAY|XDG_CURRENT_DESKTOP|WAYLAND_DISPLAY)=' | paste -sd' ')"
snapshot empty

run_round as-is

mkdir -p "$CONF_DIR"
printf '[preferred]\ndefault=gtk\n' > "$CONF"
log "wrote $CONF: default=gtk"
run_round gtk

log "done"
