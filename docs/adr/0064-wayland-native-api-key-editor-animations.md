# ADR 0064 — Focus-scoped native Wayland paste and animated API-key input (Phase 9.5.44.7)

## Context

Slint 1.18.1's Winit backend currently reaches the system clipboard through
arboard. On GNOME Mutter, the plain Wayland protocol (`wl_data_device`) is
available to a focused GUI client, but the data-control extension used by
out-of-band clipboard readers is not. X11 permission changes were explicitly
rejected. The earlier custom TextInput scroller also shifted masked bullets
when navigating the caret, and the user requested Home-style clipping fades.

## Decision

- Use the already-selected Winit/Skia backend and enable Slint's version-coupled
  `unstable-winit-030` window accessor. Inspect its raw Wayland display handle.
- Construct `smithay-clipboard` on that display and retain its connection for
  the window lifetime. This library owns a separate Wayland event queue but
  does **not** create a competing `wl_display` connection or request privileged
  `ext-data-control`. Read selection data on a worker and deliver only a validated
  ASCII token back via Slint's event loop. Discard stale requests by generation.
- In this API-token field, paste replaces the unsaved draft and moves the caret
  to the end. Normal typing, selection, and navigation remain TextInput-owned.
  No saved key is ever loaded into Slint or copied to the clipboard by Horizon.
- Remove the placeholder entirely. Animate the horizontal offset only for
  overflowing strings (165 ms; 0 ms under Reduced Motion), with 20 px nonlinear
  surface-gradient clipping masks like the existing Home selected title.
- Draw a 2 px caret at the native TextInput cursor position and animate its
  opacity through a 560 ms repeating timer (160 ms fade, disabled under Reduced
  Motion). A short string stays anchored at x = 0.
- Keep `--socket=wayland` and `backend-winit-wayland` only, without X11 sockets,
  XWayland, clipboard subprocesses, or broad Flatpak permissions.

## Validation still required

Full Cargo/Slint compilation, paste from Firefox under GNOME Wayland inside
GNOME Builder's Flatpak, Ctrl+V with existing draft, cancellation during a
pending paste, empty clipboard, a long masked key scrolled end-to-start and
back, pointer selection, Home navigation, and Reduced Motion. This patch was
prepared without the Rust toolchain or live Wayland session available to the
assistant; do not treat native clipboard behavior as confirmed until tested.

## Dependencies and credits

smithay-clipboard 0.7.3 (MIT):
https://github.com/Smithay/smithay-clipboard. See THIRD_PARTY_NOTICES.md.
