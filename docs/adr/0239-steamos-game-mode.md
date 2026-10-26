# ADR 0239 — SteamOS Game Mode: X11 fallback and a quick-menu shortcut

Status: Accepted by explicit user direction. Horizon could not start in
SteamOS Game Mode, and once running its quick menu could not be opened.

Game Mode's gamescope runs applications on Xwayland, with no Wayland display,
and Horizon was Wayland-only: Slint had only its Wayland backend and the
sandbox only the Wayland socket. Slint now also has its X11 backend (no new
crates; they were already locked) and the manifest grants
`--socket=fallback-x11`, which gives X11 only when no Wayland session exists.
Wayland desktops are unchanged and never get X11; widening this to
`--socket=x11` still needs its own decision.

Steam keeps the controller's Steam/Guide button in Game Mode, so the quick menu
had no reachable button. In-game shortcuts gain Quick menu, by default Select +
Start held together. As a pair, either button alone still reaches the game;
both together are held back. It can be reassigned or cleared like the other
shortcuts. Saved shortcut files gain it only when both buttons are free.
