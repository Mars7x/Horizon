# ADR 0072 — Single-character Wayland paste and native-caret blink

## Problem

In Phase 9.5.44.12, the asynchronous Wayland clipboard reader delivered its entire string as one Slint `WindowEvent::KeyPressed`. That event is intended for a key, not a batch of text, and could be ignored. A paste could also finish while Control was still physically pressed. The editor additionally relied on Slint's default caret blinking, which was not visible in this particular Winit/Skia build.

## Changes

- Preserve `smithay-clipboard` on the original Winit Wayland display. No X11, XWayland, or data-control.
- Keep the existing one-request-per-Ctrl+V routing, ASCII credential validation, stale-draft/session generation guards, and controller editing-target guard.
- Once paste arrives on Slint's event loop, focus the API `TextInput`, clear any pending Control/ControlR modifier state in the synthetic event stream, and deliver the pasted ASCII string as individual press/release pairs. Slint retains ownership of text insertion, caret placement, and selection replacement. Do not send the whole text as a single keyboard event or overwrite the entire draft.
- Blink the **native** Slint caret by changing `text-cursor-width` between 2px and 0px using a 540ms Timer, restarting when the text or cursor changes. Unlike the earlier independently positioned rectangle, this uses the real masked/revealed glyph metrics.
- Keep the existing editor, edge fades, input horizontal scroll, SteamGridDB preferences, and four-corner focus glow unchanged.

## Verification

Build with GNOME Builder's current Slint 1.18.1 Flatpak runtime. Test Ctrl+V once and twice, pasting into the middle of text and across a selection; test reveal/hide at several caret locations, continued caret blink, switching controller targets during an outstanding clipboard read, and canceling during a pending paste. Static validation cannot substitute for this runtime check.
