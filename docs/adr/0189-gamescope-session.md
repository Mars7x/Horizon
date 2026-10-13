# ADR 0189: Horizon as a gamescope session

Date: 2026-10-12

## Status

Proposed. Design only; see [GAMESCOPE_SESSION.md](../GAMESCOPE_SESSION.md).
Nothing is final until its spikes have run on a device.

## Context

Horizon is a desktop Flatpak. On handhelds it should also be the whole
screen, as Steam is in SteamOS's Game Mode, without writing a compositor.
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

## Consequences

- One codebase gives the desktop and the session the same pages.
- New system-bus grants exist on the desktop too, unused there.
- The manifest gains `fallback-x11`. Flatpak grants it only without
  Wayland, so the desktop's Wayland-only rule holds; the session needs an
  X11 clipboard path.
- Gamescope's Steam-shaped, thinly documented control surface is a risk;
  OpenGamepadUI is the main prior art.
- Desktop-portal features (theme "System", file chooser, links) need
  `graphical-session.target` and the GTK portal backend in the session, and
  Horizon must tag the portal's and browser's windows (Spike 4).
