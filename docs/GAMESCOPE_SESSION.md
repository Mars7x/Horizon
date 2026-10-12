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
   Steam Deck, Odin 3 and similar. **Bazzite** is the first test OS: it
   already ships gamescope-session-plus.
4. **Games run in the session's gamescope.** The per-game nested gamescope
   helper (ADR 0046, `src/session_helper.rs`) has no role in the session.
5. **Steam runs hidden** in the session so `steam://` launches, Steam Input
   and Steam's in-game overlay work; Horizon is the only face. Heroic and
   Flatpak games launch as they do today (URI; `flatpak_host`).
   **Signing in:** if Steam isn't signed in, Horizon lets Steam's login
   window come to the front once, then hides Steam again.
6. **Home mid-game opens a quick menu**: a side panel sliding in from the
   right over the dimmed game, like the Deck's quick access menu. It has:
   - Resume, and Quit game (asks first);
   - volume and brightness sliders;
   - Screenshot, saved to the Album as Horizon's own capture;
   - Performance for this game (frame limit, refresh rate);
   - a way to full Home.
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
10. **Starting:** "Horizon" is an entry on the login screen. Horizon doesn't
    set up auto-login; the OS does that, as Bazzite already does for Game
    Mode.
11. **Crashes:** `CLIENTCMD` is a small host script that restarts Horizon
    when it exits unexpectedly. A running game keeps running, because
    gamescope stays up. After a few quick failures in a row, the session
    ends for the desktop instead.
12. **Switch to Desktop** ends the session and logs in to the desktop, as on
    Bazzite and SteamOS (asks first; games are closed). Horizon doesn't run
    the OS's session switcher itself (no host command beyond `flatpak_host`):
    it exits with a "switch to desktop" exit code, and the host script runs
    the switch (on Bazzite, its `steamos-session-select`).
13. **The power button:** a short press sleeps; a long press opens Horizon's
    power menu (sleep, restart, shut down, Switch to Desktop). Horizon takes
    the key with a logind `handle-power-key` inhibitor and reads it from the
    device, so logind doesn't act on it alone while Horizon runs. Waking
   returns straight to whatever was on screen (the game, if one was running).
14. **The quick menu is session-only.** On a desktop, games are ordinary
    windows and Home brings Horizon forward, as today.
15. **Bazzite and other session-plus systems only.** SteamOS (Valve's own
    `gamescope-session`) isn't planned.

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
   Can Steam's login window be brought forward, then hidden again?
4. **Portals.** Which of OpenURI, FileChooser and Settings work in a bare
   gamescope session, and which backend (e.g. `xdg-desktop-portal-gtk`) the
   session must start.
5. **Screenshots.** Gamescope takes one on request; where does the file land,
   and can the Flatpak read it (the host's `/tmp` isn't shared)?
6. **The power key.** Does it reach Horizon as an input device on the Deck
   and the Odin 3, and does the inhibitor stop logind's own suspend?

### Where the spikes run

Until a Bazzite handheld is available, the spikes run on the development
laptop: a Lenovo Yoga Slim 7x (Snapdragon X, aarch64) with Fedora 44
Workstation (GNOME, Wayland, 4K pages).

- **Gamescope** comes from Fedora (`gamescope` 3.16.29, aarch64).
  gamescope-session-plus isn't packaged for Fedora, so a test script starts
  gamescope and Horizon the way the session file would. Session-plus itself
  is checked once Bazzite is available.
- **Nested** (gamescope as a window on GNOME): quick tries of spikes 1, 2
  and 5. GNOME's portals are still reachable from here, so it says nothing
  about spike 4.
- **Embedded** (gamescope on a text console, with nothing of GNOME behind
  it): the real check for spikes 1, 2 and 4, and for 6 with the laptop's
  power key and backlight.
- **Steam** has no ARM build. It can run through FEX (installed), so spike 3
  is tried that way, but it may behave differently from x86 Steam and is
  checked again on a handheld.
- Handheld-only details (the Deck's and Odin's power keys, their screens'
  rotation) wait for real devices.

## Phases

1. **Boot and play:** the host files; Horizon as the main app; launch and
   return from Steam, Heroic and Flatpak games; power and Switch to Desktop.
2. **Quick menu:** the side panel on Home mid-game: resume, quit game,
   volume, brightness, screenshot, performance, full Home; the power key.
3. **System settings:** Wi-Fi, Bluetooth and performance in Settings
   (session only).
4. **Packaging** for the handheld OSes (an image layer or system extension
   for immutable systems).

## Open questions

None outside the spikes above.

## References

- [ChimeraOS gamescope-session](https://github.com/ChimeraOS/gamescope-session)
  (custom clients, `STEAM_GAME`, `CLIENTCMD`)
- [OpenGamepadUI session](https://github.com/ShadowBlip/gamescope-session-opengamepadui)
  (the closest prior art: a controller UI as the session, with a quick menu;
  archived Feb 2026)
- [ArchWiki: Gamescope](https://wiki.archlinux.org/title/Gamescope)
  (`--expose-wayland`)
