# Horizon session (gamescope) — design

> **Status: design, nothing implemented.** Decisions below were made on
> 2026-10-12 (ADR 0189, Proposed). Items marked **Spike** must be tried on a
> device before the design is final.

## What it is

A login-screen choice, "Horizon", that starts **gamescope** as the whole
display with Horizon as its main app, like SteamOS's Game Mode with Horizon in
Steam's place. Games launch inside the same gamescope; Horizon decides what is
on screen. On a desktop, Horizon keeps working exactly as it does today.

Horizon does not write its own compositor or display server.

## Decisions

1. **One app.** The session runs the same Flatpak (`flatpak run
   io.github.Mars7x.Horizon --session`). Parity comes from sharing the code,
   not from keeping two builds in step. `--session` is the only switch;
   Horizon doesn't guess from the environment.
2. **gamescope-session-plus** (ChimeraOS's framework, shipped by Bazzite)
   starts gamescope: display size, rotation, HDR, VRR and failed-start
   recovery come from it. Horizon's host package adds two files:
   - `/usr/share/gamescope-session-plus/sessions.d/horizon` (sets
     `CLIENTCMD`, the command that starts Steam hidden and then Horizon);
   - `/usr/share/wayland-sessions/gamescope-session-horizon.desktop` (the
     login-screen entry, `DesktopNames=gamescope`).
3. **Targets handhelds first** (built-in screen, battery, sleep, brightness):
   Steam Deck, Odin 3 and similar. The OS to test on is still open.
4. **Games run in the session's gamescope.** The per-game nested gamescope
   helper (ADR 0046, `src/session_helper.rs`) has no role in the session.
5. **Steam runs hidden** in the session so `steam://` launches, Steam Input
   and Steam's in-game overlay work; Horizon is the only face. Heroic and
   Flatpak games launch as they do today (URI; `flatpak_host`).
6. **Home mid-game opens a quick menu** over the paused game: resume, quit
   game, volume, brightness, and a way to full Home.
7. **Session-only system controls:** power (sleep, restart, shut down, Switch
   to Desktop), volume and brightness, Wi-Fi and Bluetooth, and performance
   (frame limit, refresh rate, scaling; TDP needs root and comes later).
8. **New Flatpak grants, narrow names only:** `--system-talk-name` for
   `org.freedesktop.login1` (power, brightness),
   `org.freedesktop.NetworkManager` and `org.bluez`, each explained in the
   manifest and an ADR like the `flatpak_host` grant. Grants are static, so
   they exist on the desktop too; Horizon uses them only in the session.
9. **One owner per boundary**, as with `flatpak_host.rs`: all gamescope
   control in `src/platform/gamescope.rs`; all system services in their own
   `platform` modules. Presentation asks a `SessionCapabilities` value what
   exists and hides what doesn't, so pages never check "am I the session".

## How gamescope is controlled

Gamescope's embedded mode is built around Steam, and it is steered with X11
window properties, not Wayland:

| Property | Where | Meaning |
| --- | --- | --- |
| `STEAM_GAME = 769` | Horizon's main window | The session's main app. Without a `STEAM_GAME`, gamescope may show nothing. |
| `STEAM_GAME = <other>` | A game's window | Drawn over the main app; the main app returns when it closes. Steam sets its games' app ID. |
| `GAMESCOPECTRL_BASELAYER_APPID` | Root window | Which app is in front; how the quick menu's "Home" and "Resume" switch. |
| `STEAM_OVERLAY` / `GAMESCOPE_EXTERNAL_OVERLAY` | An overlay window | Drawn over the game (the quick menu). Little documented. |
| `STEAM_INPUT_FOCUS` | — | Which window gets input while an overlay is up. |

Gamescope accepts Wayland apps only with `--expose-wayland`, and focus for
them is still being patched around in other projects.

**You chose: try Wayland first (Spike 1).** Run Horizon as a Wayland app with
`--expose-wayland` and see whether it can be the main app, give way to games,
come back, and show an overlay. The likely catch: the root-window properties
above are X11 either way, so Horizon may still need an X11 connection just to
set them. If Wayland can't do it, the fallback is the `fallback-x11` socket
(X11 only where there is no Wayland, so the desktop stays Wayland-only) and
Slint's X11 backend in the session.

## Feature parity

| Feature | Desktop | Session |
| --- | --- | --- |
| Home, Library, Activity, Achievements, Album, Shop, Profile | ✓ | ✓ same code |
| Steam games | `steam://` to the desktop's Steam | `steam://` to the hidden Steam (Spike 3) |
| Heroic games | `heroic://` | same; Heroic's own window may show (Spike 2) |
| Flatpak games | `flatpak run` via `flatpak_host` | same; window tagging (Spike 2) |
| Playtime | as today | as today; the session can also see what's in front |
| Controllers, glyphs, swap | SDL | same (SDL reads devices, not windows) |
| On-screen keyboard | Horizon's own | same (a touch screen may arrive as a mouse; the field's keyboard button covers it) |
| Theme "System", clock format | desktop settings portal | no desktop behind it: fall back to defaults, hide "System" (Spike 4) |
| Choose a profile picture | file chooser portal | needs a portal backend in the session (Spike 4) |
| Opening links | OpenURI portal | needs a backend in the session (Spike 4) |
| Paste in text fields | Wayland clipboard | depends on Spike 1 |
| Quick menu, power, volume, brightness, Wi-Fi, Bluetooth, performance | — | ✓ |
| Battery, network, controller status | ✓ | ✓ |

## Spikes (on a device, before building)

1. **Wayland or X11.** Can a Wayland Horizon be gamescope's main app, give way
   to games and come back, and draw an overlay? Can it set the root
   properties without X11?
2. **Non-Steam windows.** Does gamescope show a Heroic or Flatpak game whose
   window has no `STEAM_GAME`? If not, Horizon tags new game windows itself.
   Does Heroic's own window appear when a game starts?
3. **Hidden Steam.** Does `steam -silent` stay hidden in embedded gamescope
   (login, updates, crash dialogs)? Does it write
   `GAMESCOPECTRL_BASELAYER_APPID` itself and fight Horizon? Who gets the
   Guide button in a Steam game: Steam's overlay or Horizon's quick menu?
4. **Portals.** Which of OpenURI, FileChooser and Settings work in a bare
   gamescope session, and which backend (e.g. `xdg-desktop-portal-gtk`) the
   session must start.

## Phases

1. **Boot and play:** the host files; Horizon as the main app; launch and
   return from Steam, Heroic and Flatpak games; power and Switch to Desktop.
2. **Quick menu:** the overlay on Home mid-game: resume, quit game, volume,
   brightness, full Home.
3. **System settings:** Wi-Fi, Bluetooth and performance in Settings
   (session only).
4. **Packaging** for the handheld OSes (an image layer or system extension
   for immutable systems).

## Open questions

- Which OS to test on first (Bazzite, SteamOS, Fedora on an Odin 3).
- SteamOS ships Valve's own `gamescope-session`, not session-plus: support it
  with our own small script later, or not at all?
- Album captures in the session: gamescope can take screenshots; how Horizon
  asks for one.
- Sleep and resume on handhelds: who handles the power button (logind, the
  OS, InputPlumber) and what Horizon shows on wake.

## References

- [ChimeraOS gamescope-session](https://github.com/ChimeraOS/gamescope-session)
  (custom clients, `STEAM_GAME`, `CLIENTCMD`)
- [OpenGamepadUI session](https://github.com/ShadowBlip/gamescope-session-opengamepadui)
  (the closest prior art: a controller UI as the session, with a quick menu;
  archived Feb 2026)
- [ArchWiki: Gamescope](https://wiki.archlinux.org/title/Gamescope)
  (`--expose-wayland`)
