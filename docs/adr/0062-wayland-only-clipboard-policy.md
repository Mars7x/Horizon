# ADR 0062 — Wayland-only Settings clipboard policy (Phase 9.5.44.5)

## Context

Phase 9.5.44.4 added a Flatpak `--socket=x11` permission to work around a
GNOME Mutter clipboard incompatibility in Slint 1.18.1. The user explicitly
rejected X11 and XWayland workarounds. The reported API-key paste problem is
not proof that the X11 permission fixed anything.

Slint 1.18.1's winit clipboard implementation uses arboard. On Wayland it
relies on data-control; on desktops without that protocol it can try X11.
GNOME Mutter does not reliably expose the required data-control interfaces.
Simply enabling `--socket=x11` widens sandbox access and is not an acceptable
solution to a native Wayland application. Using an independent headless
clipboard reader is not a drop-in replacement either: ordinary Wayland
clipboard access is gated by keyboard focus.

## Decision

- Drop the Flatpak `--socket=x11` permission; grant `--socket=wayland` only.
- Disable Slint's `backend-winit-x11` feature. Keep the existing Skia renderer
  and the rest of Horizon's dependencies and permissions unchanged.
- Do not reintroduce `--socket=fallback-x11`, shelling out to `xclip`/`xsel`,
  XWayland-based clipboard helpers, or broad host execution as fallbacks.
- Keep the slim API-key editor, eye reveal icon, native Slint `TextInput`,
  settings persistence, and SteamGridDB artwork behavior untouched.
- Treat API-key Ctrl+V paste on GNOME Wayland as **not yet fixed**. It needs a
  tested implementation using the focused window's core `wl_data_device`
  protocol integrated with the Wayland event loop, or a suitable upstream
  Slint/winit change. Do not claim Clipboard Portal support unless established
  for ordinary app paste on the target runtime.

## Verification required

1. Rebuild the Flatpak in GNOME Builder and confirm Horizon starts under native
   Wayland, without X11 socket access.
2. Verify keyboard typing, eye reveal, Save, Cancel and controller navigation.
3. Treat Ctrl+V from Firefox as an open functional bug until it works in the
   actual sandbox. Never silently substitute X11 for this test.
4. Recheck the generated Cargo dependency feature graph and Flatpak permissions
   before release, including any transitive X11 crates.

This ADR supersedes the X11 permission decision in ADR 0061, without editing
its historical record. No runtime clipboard fix is claimed in this phase.
