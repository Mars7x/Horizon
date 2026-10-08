# ADR 0061 — Restore native text editing and icon-only reveal (Phase 9.5.44.4)

## Context

Phase 9.5.44.3 intercepted Ctrl+V in the Slint `TextInput`, then called an
out-of-band Rust clipboard reader. Users reported empty paste and misleading
feedback. The Rust reader relied on Wayland data-control or X11, while the
Flatpak manifest granted only `fallback-x11` alongside Wayland. GNOME's Mutter
does not generally provide the wlroots data-control extension, and the fallback
X11 socket is hidden from a process with Wayland access.

The explicit Paste button added no capability (it invoked the same failing
reader), occupied excessive field width, and redirected focus. The placeholder
was independently painted over the editable cursor, producing a misplaced
caret when the field was empty.

## Decision

1. Remove the extra Paste button, Rust clipboard adapter and its public Slint
   callback. Let the standard `TextInput` handle Ctrl+V, Ctrl+A, selection,
   undo and paste at the current cursor position. Keep Esc/Back cancel and
   Enter/Accept save.
2. Permit `--socket=x11` together with `--socket=wayland` for the Slint 1.18
   XWayland clipboard fallback. This **widens X11 access** beyond the previous
   fallback-only grant; it does not grant filesystem, session-bus or arbitrary
   host execution access. We prefer a fully Wayland-native clipboard once
   supported without relying on XWayland. Release security review must revisit
   the need for this grant.
3. Replace Show/Hide text with an accessible eye/eye-slash vector button. A
   saved key is never read into the editor; only the temporary draft can be
   shown. Keep the button within the existing input border.
4. Use the hint `Enter an API key` with its beginning to the right of the
   initial caret, reset horizontal scrolling for empty drafts, and position
   the cursor at the start when opening an editor session.

No change to API key persistence, API requests, artwork selection, or cache.

## Validation required in GNOME Builder Flatpak

- Copy an API key from Firefox and paste using Ctrl+V on a GNOME Wayland session.
- Verify paste at cursor and replacing selection (Ctrl+A then Ctrl+V).
- Confirm the empty hint/caret do not overlap, including after a blank paste.
- Toggle draft visibility with the eye icon; ensure saved keys remain masked.
- Save, cancel, reopen; verify draft is cleared and no clipboard text is logged.
- Test controller Accept/Back and Reduced Motion.

Rust/Slint compilation and live clipboard access were not available in the
patch-authoring environment. These checks remain unverified until an app build.