# ADR 0063 — Stable API key input layout (Phase 9.5.44.6)

## Reported behaviour

In the SteamGridDB key editor, moving the caret left in a three-character
masked key visibly shifted the password bullets. The dialog also displayed an
unnecessary Ctrl+V / Enter / Esc instructional strip.

## Resolution

- Stop the custom `TextInput` horizontal scroll offset from ever becoming
  positive. The input's left inset is fixed; scrolling to see the right end of
  long keys only translates the text to the left.
- Restore the scroll offset to zero for empty or short drafts. This also
  prevents leftover scroll from a long key after deleting text.
- Remove the keyboard-hint strip; retain the Cancel and Save key actions.
- Shorten the modal from 274px to 246px and reflow error feedback and buttons.
- Keep the saved API key out of Slint; the eye icon reveals only the unsaved
  draft, unchanged from 9.5.44.5.

## Separate open issue: GNOME Wayland clipboard

This patch **does not** change clipboard integration or claim native paste
is working. Slint 1.18.1's winit clipboard uses `arboard`, which does not
implement the focus-scoped `wl_data_device` path needed for Mutter without
X11. Fixing that correctly requires changes to the existing winit/Slint
window-event integration, not a parallel, unfocused clipboard reader.

No X11 permissions, XWayland fallback, clipboard subprocess, or remote
portal privileges were added. Preserve ADR 0062's Wayland-only policy.

## Verification

Rebuild in GNOME Builder and test: a three-letter password; Left/Right/Home/
End; deletion from a long value; eye toggle; focus after reopening; and the
absence of the hint strip. Clipboard paste is still an outstanding test/fix.
