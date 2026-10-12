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

## Consequences

- One codebase gives the desktop and the session the same pages.
- New system-bus grants exist on the desktop too, unused there.
- The manifest gains `fallback-x11`. Flatpak grants it only without
  Wayland, so the desktop's Wayland-only rule holds; the session needs an
  X11 clipboard path.
- Gamescope's Steam-shaped, thinly documented control surface is a risk;
  OpenGamepadUI is the main prior art.
- Desktop-portal features (theme "System", file chooser, links) need a
  backend in the session or a fallback.
