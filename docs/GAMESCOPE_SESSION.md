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
   window come to the front once, then hides Steam again (base layer 769,
   then back to Horizon's own ID; Spike 3).
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
| `STEAM_GAME = <Horizon's ID>` | Horizon's main window | Horizon's own app ID, not 769: 769 is Steam's, and Steam tags its own windows with it (Spike 3). An untagged window is never shown (Spikes 1, 2). |
| `STEAM_GAME = 769` | Steam's windows | Set by Steam. Shown only while the base layer is 769, e.g. for Steam's login. |
| `STEAM_GAME = <game>` | A game's window | Steam tags its games; Horizon tags Heroic and Flatpak games itself (Spike 2). |
| `GAMESCOPECTRL_BASELAYER_APPID` | Root window | Which app is shown. Steam never sets it in this session; Horizon sets it to a game's ID when its window appears and back to its own ID when it closes. Nothing is shown while it names an app with no window (Spike 3). Also how the quick menu's "Home" and "Resume" switch. |
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

**Spike 1 result (nested, main PC, 2026-10-12): Wayland fails in Steam mode.**
With `-e`, as session-plus starts gamescope, a Wayland window is never shown:
not Horizon, not a Wayland test window, whatever its app ID. In Steam mode
gamescope shows only a window whose app ID matches the root's
`GAMESCOPECTRL_BASELAYER_APPID`, and a Wayland window always counts as
app ID 0. `SteamAppId` (769 or 4242) reaches gamescope's Vulkan layer but
doesn't change that; `SteamGameId` isn't read. An X11 window tagged
`STEAM_GAME` is shown once the base layer names its app ID, and nothing is
shown after it closes. Without `-e` everything works: Horizon is shown, a
Wayland or X11 window on top takes the screen, and Horizon returns when it
closes. This points to the X11 fallback.

**Embedded (text console, same PC, 2026-10-12):** Steam mode gives the same
result as nested. Without `-e`, Horizon is shown and returns after X11
windows close, but while a second Wayland window is open gamescope focuses
nothing and the screen shows neither window. Only X11 windows behave the
same in every mode. Still to confirm: whether session-plus can run without
`-e` (that would also drop Steam integration), and overlays.

**You chose (after Spike 1): X11 in the session, Wayland wherever it
works.** The desktop stays Wayland-only. In the session, Horizon's window is
X11, because gamescope only shows tagged X11 windows; if a later gamescope
shows Wayland windows in Steam mode, the session moves back to Wayland. To
build it:

- Slint gains `backend-winit-x11`; Wayland stays the first choice.
- The manifest gains `--socket=fallback-x11`, which Flatpak grants only when
  no Wayland display is visible, so the desktop never gets X11. Its
  "Wayland-only UI" comment changes to say so.
- The session starts Horizon without a Wayland display (no
  `--expose-wayland`, or `WAYLAND_DISPLAY` unset by the host script), so
  the X11 socket is the one Horizon gets.
- The clipboard in the session needs an X11 path; `wayland_clipboard` is
  Wayland-only.
- Native Wayland games would be hidden too; spike 2 checks how games are
  kept on X11.

## Feature parity

| Feature | Desktop | Session |
| --- | --- | --- |
| Home, Library, Activity, Achievements, Album, Shop, Profile | ✓ | ✓ same code |
| Steam games | `steam://` to the desktop's Steam | `steam://` to the hidden Steam (Spike 3) |
| Heroic games | `heroic://` | same; Horizon tags the game's window (Heroic's own stays hidden) |
| Flatpak games | `flatpak run` via `flatpak_host` | same, passing gamescope's display; Horizon tags the window |
| Playtime | as today | as today; the session can also see what's in front |
| Controllers, glyphs, swap | SDL | same (SDL reads devices, not windows) |
| On-screen keyboard | Horizon's own | same (a touch screen may arrive as a mouse; the field's keyboard button covers it) |
| Theme "System", clock format | desktop settings portal | no desktop behind it: fall back to defaults, hide "System" (Spike 4) |
| Choose a profile picture | file chooser portal | needs a portal backend in the session (Spike 4) |
| Opening links | OpenURI portal | needs a backend in the session (Spike 4) |
| Paste in text fields | Wayland clipboard | needs an X11 clipboard path |
| Quick menu, power, volume, brightness, Wi-Fi, Bluetooth, performance | — | ✓ |
| Battery, network, controller status | ✓ | ✓ |

## Spikes (on a device, before building)

1. **Wayland or X11.** Can a Wayland Horizon be gamescope's main app, give way
   to games and come back, and draw an overlay? Can it set the root
   properties without X11? **Nested result: not in Steam mode** (see
   [How gamescope is controlled](#how-gamescope-is-controlled)). Script:
   `scripts/spikes/gamescope-spike1.sh [steam|plain]`, which logs
   `gamescopectl focus_info` and one frame from gamescope's PipeWire stream
   at each step (`gamescopectl screenshot` saved nothing on 3.16.29).
   Embedded: the same, and a second Wayland window isn't shown even
   without `-e`. Overlay still to do.
2. **Non-Steam windows.** Does gamescope show a Heroic or Flatpak game whose
   window has no `STEAM_GAME`? If not, Horizon tags new game windows itself.
   Does Heroic's own window appear when a game starts?
   **Result (main PC, nested and embedded, 2026-10-12):** no. In Steam mode
   an untagged window is never shown: Dolphin, Azahar, Highscore and Heroic
   all stayed hidden until another X11 client set `STEAM_GAME` on their
   window and the base layer named it; then each was shown, and nothing
   after it closed. So Horizon tags new game windows itself, and Heroic's own
   window shows only if tagged. Heroic detects gamescope by itself and goes
   fullscreen. With no Wayland display exposed, the Wayland-capable apps
   (Azahar, Highscore) fell back to X11 by themselves. **Launch route:**
   `flatpak-spawn --host` (today's `flatpak_host`) runs the game with the
   Flatpak session helper's environment (`DISPLAY=:0`,
   `WAYLAND_DISPLAY=wayland-0` from the desktop), not Horizon's, so a game
   launched that way never reached gamescope. In the session, `flatpak_host`
   must pass gamescope's display along. Not yet tried: a game launched
   through Heroic (`HEROIC_GAME=<appName>`). Script:
   `scripts/spikes/gamescope-spike2.sh [session|expose]`.
3. **Hidden Steam.** Does `steam -silent` stay hidden in embedded gamescope
   (login, updates, crash dialogs)? Does it write
   `GAMESCOPECTRL_BASELAYER_APPID` itself and fight Horizon? Who gets the
   Guide button in a Steam game: Steam's overlay or Horizon's quick menu?
   Can Steam's login window be brought forward, then hidden again?
   **Result (main PC, Flatpak Steam 1.0.0.87, nested and embedded,
   2026-10-12):** `steam -silent` stayed hidden (no window shown in 90 s;
   Steam was already signed in, so no login or update window came up). Steam
   never set `GAMESCOPECTRL_BASELAYER_APPID`. `steam://rungameid` reached the
   hidden Steam both from gamescope and through the OpenURI portal
   (Horizon's route); the game window opened in 6–10 s, tagged with its app
   ID by Steam, and was shown only once the base layer named it. After the
   game closed, nothing was shown until the base layer was set back to the
   main app. Steam tags its own windows 769, so a main app tagged 769 lost
   the screen to Steam's main window; with its own ID, base layer 769 brought
   Steam's main window forward and the main app's ID hid it again, the
   mechanism for signing in. A game closed cleanly on `WM_DELETE_WINDOW`.
   **Guide button** (embedded, 8BitDo Ultimate 2, A Short Hike): pressing
   it opened nothing; no Steam overlay and no overlay window in gamescope.
   Steam's controller log shows Steam Input following gamescope's focused
   app (the game, the main app's own ID, 769), so Steam Input will also
   apply to Horizon's window; check how Horizon sees the controller then.
   The Guide button is free for Horizon's quick menu in this setup.
   **Still open:** Steam's login and update windows (Steam was signed in).
   Script: `scripts/spikes/gamescope-spike3.sh`.
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
- **The main PC** (Fedora Silverblue, x86_64, AMD graphics) runs the same
  spikes with real x86 Steam, so spike 3 is answered there. Gamescope is
  layered onto the image (`rpm-ostree install gamescope`), as the session
  needs it on the host, not in a toolbox. Steam is the Flathub Flatpak, so
  spike 3 also covers Flatpak Steam: `steam://` reaching it, and its games
  being tagged for gamescope from inside its sandbox. Gamescope there is
  3.16.29 (Fedora 44), layered and booted on 2026-10-12.
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
