# ADR 0110 — Hide Heroic's window through its supported launch URI

- Status: Accepted (Phase 9.5.44.53)
- Applies to: Heroic/Epic (Legendary) external launches

## Context

Horizon previously passed `heroic://launch?appName=...&runner=legendary`
through the existing portal OpenURI executor. Heroic could display its main
window, pulling attention away from the console-first Horizon interface.
Historically Heroic supported `--no-gui` on its CLI, but OpenURI cannot attach
process arguments. Giving a Flatpak broad host process execution for this is
unnecessary and violates the source boundary.

Heroic upstream PR #5501 was merged on May 15, 2026 and shipped in 2.22.0.
Its supported per-URI `gui=false` flag hides the window at startup, on second
instance invocation, and when an existing Heroic process handles the URL.

## Decision

The Heroic adapter adds `&gui=false` after the encoded `appName` and `runner`
query parameters on `SourceLaunchTarget::Uri`. Keep the same normal source
launch and XDG OpenURI pipeline; do not use direct `flatpak run`, shell scripts,
private environment variables, a host helper, or source-specific service logic.
Heroic owns all actual game startup and remains free to display prompts when
user interaction is required.

## Consequences

- Heroic 2.22.0+ hides its window during normal game launches initiated by
  Horizon; this works even if Heroic was already running.
- Older Heroic builds may ignore the flag and continue opening the window.
  Horizon cannot guarantee a hidden window with those versions without a
  separate launch path or changing Heroic.
- This does **not** make Heroic process-free, guarantee game execution,
  or make launch-log-based Playing observation authoritative. These are
  separate source concerns.
- No changes to permissions, protocols, migrations, dependencies, Steam,
  lifetime-playtime imports, UI, or manual settings.

## References

- https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/pull/5501
- https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/releases/tag/v2.22.0
