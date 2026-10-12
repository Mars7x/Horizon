# ADR 0181: The Shop browses Flathub and manages Flatpaks through the host

Date: 2026-10-11

## Status

Accepted. A deliberate exception to "no host execution" (AGENTS.md).

## Context

The Shop utility was a placeholder. It should be a Flatpak shop: Flathub's
apps, browsable with a controller, installed and managed from Horizon. The
sandbox cannot install apps. The alternatives were handing Install to the
desktop's software store (no new permission, but Horizon can't manage
anything and the store is a desktop window, poor under gamescope), or the
optional host session helper (narrow, but not installed for most users).

## Decision

- The Shop browses and searches **all of Flathub**, games first, filtered
  to this machine's architecture, through Flathub's public API.
- Horizon gets **`--talk-name=org.freedesktop.Flatpak`** and runs the
  host's `flatpak` through `flatpak-spawn --host` to list, install, update
  and uninstall apps. This is host command execution; it is confined:
  - every host command is in `src/platform/flatpak_host.rs`;
  - only fixed `flatpak` subcommands, never a shell, program, path or
    argument list from elsewhere;
  - the app ID is checked first (reverse DNS, plain characters, starting
    with a letter so it can't be read as an option);
  - Horizon never uninstalls itself.
- Installs go system-wide when the system has Flathub, else per user
  (adding Flathub to the user installation), so it works without a password
  prompt where polkit allows.
- Jobs run one at a time, with flatpak's own progress when it prints any.
- Without host access, Install opens the software store on the app.
- The placeholder page component (`UtilitySubmenuPage`, `PagePlaceholder`)
  is removed: Shop was its last user.

## Consequences

- Horizon can run commands on the host. Flathub review usually questions
  this permission; publishing there would need to justify it or drop it.
- Installed apps don't join the Library yet; that would be a Flatpak game
  source of its own.
