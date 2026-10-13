#!/usr/bin/env bash
# Try Horizon's gamescope session on a machine without gamescope-session-plus
# (docs/GAMESCOPE_SESSION.md). Does what session-plus would: a session target
# bound to graphical-session.target, gamescope in Steam mode, and
# data/session/horizon-gamescope-session as its client.
#
# Log out of the desktop, log in on a text console (Ctrl+Alt+F3), then:
#
#   bash scripts/session-dev.sh          # the installed Horizon Flatpak
#   bash scripts/session-dev.sh --dev    # this checkout's debug build, run in
#                                        # the Freedesktop 26.08 SDK (see
#                                        # docs/DEVELOPMENT.md; build it first)
#
# To end the session, switch to another console and run:
#
#   bash scripts/session-dev.sh --stop
#
# At the end, also when interrupted, it removes the session target and
# restores the user's systemd environment. Log:
# ~/horizon-spikes/session-dev-<time>.log.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET=horizon-dev-session.target
TARGET_FILE="$HOME/.config/systemd/user/$TARGET"
DEV="$HOME/.cache/horizon-dev"

case "${1:-}" in
    --stop)
        # Horizon quits on SIGTERM; the session then ends normally.
        pkill -TERM -x horizon && echo "asked Horizon to quit" || echo "Horizon isn't running"
        exit 0
        ;;
    --run-dev)
        # Called by the session client as HORIZON_RUN.
        shift
        X="$DEV/xdg"
        mkdir -p "$X"/{data,config,cache,state}
        exec flatpak run --socket=x11 --device=dri --device=input --share=network \
            --filesystem=home --filesystem=xdg-run/pipewire-0 \
            --talk-name=org.freedesktop.Flatpak \
            --env=DISPLAY="$DISPLAY" --env=LD_LIBRARY_PATH="$DEV/sdl3/lib64" \
            --env=RUST_LOG="${RUST_LOG:-horizon=debug}" \
            --env=XDG_DATA_HOME="$X/data" --env=XDG_CONFIG_HOME="$X/config" \
            --env=XDG_CACHE_HOME="$X/cache" --env=XDG_STATE_HOME="$X/state" \
            --command="$ROOT/target/debug/horizon" org.freedesktop.Sdk//26.08 "$@"
        ;;
    --dev | "") ;;
    *)
        echo "Usage: $0 [--dev | --stop]" >&2
        exit 2
        ;;
esac

if pgrep -u "$USER" -x gnome-shell >/dev/null || [[ -n "${WAYLAND_DISPLAY:-}${DISPLAY:-}" ]]; then
    echo "A desktop is running. Log out, log in on a text console, and run this there." >&2
    exit 1
fi
if flatpak ps --columns=application | grep -qx com.valvesoftware.Steam || pgrep -x steam >/dev/null; then
    # It would keep its old display; the session starts its own, hidden.
    echo "Steam is running. Quit it first (it may have outlived the desktop)." >&2
    exit 1
fi
if [[ -e "$TARGET_FILE" ]]; then
    if systemctl --user is-active --quiet "$TARGET"; then
        echo "Another dev session is running." >&2
        exit 1
    fi
    rm -f "$TARGET_FILE"   # left by a run that was killed
fi
if [[ "${1:-}" == --dev ]]; then
    [[ -x "$ROOT/target/debug/horizon" ]] || { echo "Build the debug binary first." >&2; exit 1; }
    export HORIZON_RUN="bash $ROOT/scripts/session-dev.sh --run-dev"
fi

mkdir -p "$HOME/horizon-spikes"
LOG="$HOME/horizon-spikes/session-dev-$(date +%Y%m%d-%H%M%S).log"
ENV_BEFORE=$(systemctl --user show-environment)

cleanup() {
    systemctl --user stop "$TARGET"
    rm -f "$TARGET_FILE"
    systemctl --user daemon-reload
    for var in DISPLAY XDG_CURRENT_DESKTOP XDG_SESSION_TYPE WAYLAND_DISPLAY XDG_DESKTOP_PORTAL_DIR; do
        old=$(grep "^$var=" <<< "$ENV_BEFORE" | head -1)
        if [[ -n "$old" ]]; then
            systemctl --user set-environment "$old"
        else
            systemctl --user unset-environment "$var"
        fi
    done
    systemctl --user stop flatpak-session-helper.service xdg-desktop-portal.service 2>/dev/null
    echo "Session ended; restored the systemd environment. Log: $LOG"
}
# Also when the run is interrupted (Ctrl+C, a lost console, SIGTERM).
trap cleanup EXIT
trap 'exit 130' INT HUP TERM

mkdir -p "$(dirname "$TARGET_FILE")"
cat > "$TARGET_FILE" <<'UNIT'
[Unit]
Description=Horizon dev session (temporary, scripts/session-dev.sh)
BindsTo=graphical-session.target
Before=graphical-session.target
UNIT
systemctl --user daemon-reload
systemctl --user start "$TARGET"

echo "Log: $LOG"
gamescope --steam --backend drm -- "$ROOT/data/session/horizon-gamescope-session" > "$LOG" 2>&1
echo "gamescope exit: $?" >> "$LOG"
