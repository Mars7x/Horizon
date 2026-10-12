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
- Wayland is tried first inside gamescope; X11 through `fallback-x11` is the
  fallback if gamescope's focus and overlay control need it.

## Consequences

- One codebase gives the desktop and the session the same pages.
- New system-bus grants exist on the desktop too, unused there.
- Gamescope's Steam-shaped, thinly documented control surface is a risk;
  OpenGamepadUI is the main prior art.
- Desktop-portal features (theme "System", file chooser, links) need a
  backend in the session or a fallback.
