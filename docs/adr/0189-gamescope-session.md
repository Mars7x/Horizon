# ADR 0189: Horizon as a gamescope session

Date: 2026-10-12

## Status

Superseded (2026-10-12). The session was cancelled during Phase 1 in favour
of embedding emulator cores in the desktop Flatpak; its code was removed.
Kept as history; see [GAMESCOPE_SESSION.md](../GAMESCOPE_SESSION.md).

## Context

Horizon is a desktop Flatpak. On a PC with a controller or on a handheld it
should also be able to be the whole screen, as Steam is in SteamOS's Game
Mode, without writing a compositor. No device is the target; hardware
controls appear where the hardware exists.
Gamescope already does this job and is steered by X11 window properties that
Steam sets.

## Decision

- The session is gamescope, started by gamescope-session-plus, running the
  same Horizon Flatpak with `--session`. A small host package adds the session
  file and the login-screen entry.
- Games run inside the session's gamescope; ADR 0046's nested per-game
  helper has no role there.
- Steam runs hidden in the session for Steam games, Steam Input and its
  overlay.
- Home during a game opens a quick-menu overlay.
- Session-only system controls (power, volume, brightness, Wi-Fi, Bluetooth,
  performance) use narrow `--system-talk-name` grants for login1,
  NetworkManager and BlueZ.
- All gamescope control lives in `platform::gamescope`; pages ask a
  `SessionCapabilities` value what exists.
- The host script that starts Horizon also restarts it after a crash, and
  performs Switch to Desktop when Horizon exits with that exit code, so the
  Flatpak gains no host command beyond `flatpak_host`.
- Bazzite is the first test OS.
- ~~Wayland is tried first inside gamescope; X11 through `fallback-x11` is the
  fallback if gamescope's focus and overlay control need it.~~ Revised after
  Spike 1 (2026-10-12): in the session Horizon's window is X11, through
  `--socket=fallback-x11` and Slint's X11 backend, because gamescope shows
  only tagged X11 windows in Steam mode. The session starts Horizon without a
  Wayland display so Flatpak grants that socket; the desktop stays
  Wayland-only. Wayland is used wherever it works, and the session returns to
  it if gamescope gains Wayland app IDs.
- Decided after the spikes (2026-10-12): Horizon's app IDs are a reserved
  block from `0x7F000000` (Horizon itself, then base + database `id` for
  Heroic and Flatpak games); the session's host script provides
  gamescope's `DISPLAY` to the user's systemd environment so
  `flatpak_host` stays unchanged (checked: Flatpak games launched through
  it then open in gamescope); screenshots go through gamescope's
  control socket (`--filesystem=xdg-run/gamescope-0`).

## Spike 1 result (2026-10-12)

Nested on the main PC (gamescope 3.16.29): with `-e`, gamescope shows only
a window whose app ID matches the root's `GAMESCOPECTRL_BASELAYER_APPID`.
Wayland windows always count as app ID 0, so a Wayland Horizon is never
shown in Steam mode; an X11 window tagged `STEAM_GAME` is. Without `-e`, a
Wayland Horizon works and returns after a window on top closes. This points
to the X11 fallback.

Embedded on the same PC: Steam mode is the same. Without `-e`, a second
Wayland window leaves gamescope with no focus while it is open; X11 windows
work in every mode. The Wayland/X11 decision above is revised accordingly.
See [GAMESCOPE_SESSION.md](../GAMESCOPE_SESSION.md).

## Spike 2 result (2026-10-12)

Untagged non-Steam windows (Dolphin, Azahar, Highscore, Heroic) are never
shown in Steam mode; tagged by another X11 client, they are. Horizon will
tag game windows itself. With no Wayland display exposed, Wayland-capable
apps use X11 on their own. `flatpak-spawn --host` gives games the desktop's
display, so in the session `platform::flatpak_host` must pass gamescope's.

## Spike 3 result (2026-10-12)

Flatpak Steam started `-silent` stays hidden and never sets the base layer.
`steam://` launches reach it from gamescope and through OpenURI; Steam tags
the game window, but the game is shown only once the base layer names it,
and nothing is shown after it closes until the base layer is set back.
Steam tags its own windows 769, so Horizon takes its own app ID (not 769)
and drives the base layer itself: the game's ID when its window appears,
its own when it closes, 769 to show Steam's login. Pressing Guide in a
Steam game opened nothing, so the button is free for Horizon's quick menu;
Steam Input follows gamescope's focused app, Horizon's window included.

## Overlay spike result (2026-10-12)

The quick menu can be a full-screen, mostly transparent X11 window tagged
`STEAM_OVERLAY=1` and `STEAM_INPUT_FOCUS=1`: gamescope draws it over the
running game, keeps drawing the game, and gives the overlay input.
Panel-sized overlay windows and `GAMESCOPE_EXTERNAL_OVERLAY` were not drawn.
Whether a game also reacts to the controller while it is up is still open.

## Spike 4 result (2026-10-12)

With GNOME logged out, portals worked in gamescope once
`graphical-session.target` was active (the portal service requires it):
the GTK backend was chosen without configuration, Settings returned the
colour scheme and clock format, and FileChooser and OpenURI opened their
windows. Those windows are hidden until Horizon tags them, like games'.
The theme "System" setting can stay in the session.

## Spike 5 result (2026-10-12)

Gamescope screenshots work with a window on screen and leave out the
quick-menu overlay. `gamescopectl screenshot <path>` writes a PNG anywhere;
the root-property request always writes the host's `/tmp/gamescope.png`,
which the Flatpak cannot read. How Horizon requests a screenshot it can
read (gamescope's control socket, or another route) is still to decide.

## Spike 6 result (2026-10-12)

The power button's devices aren't readable by the user, but embedded
gamescope delivers the key to the focused window as `XF86PowerOff`. A PC's
ACPI button reports no hold time, so the power key opens the power menu
there; where the hold time is reported, a short press sleeps and a long
press opens the menu. Horizon still holds a `handle-power-key` inhibitor so
logind doesn't power off.

## Consequences

- One codebase gives the desktop and the session the same pages.
- New system-bus grants exist on the desktop too, unused there.
- The manifest gains `--filesystem=xdg-run/gamescope-0` for the
  screenshot control socket.
- The manifest gains `fallback-x11`. Flatpak grants it only without
  Wayland, so the desktop's Wayland-only rule holds; the session needs an
  X11 clipboard path.
- Gamescope's Steam-shaped, thinly documented control surface is a risk;
  OpenGamepadUI is the main prior art.
- Desktop-portal features (theme "System", file chooser, links) need
  `graphical-session.target` and the GTK portal backend in the session, and
  Horizon must tag the portal's and browser's windows (Spike 4).
