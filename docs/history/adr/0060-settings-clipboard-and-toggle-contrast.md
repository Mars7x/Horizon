# ADR 0060 — Reliable API-key paste and compact editor (Phase 9.5.44.3)

## Decision

Route both Ctrl+V on the focused masked `TextInput` and the visible Paste button
through a single Slint callback to `SettingsController`. Native clipboard text
is read using arboard with Wayland data-control/X11 support, followed by the
existing SDL3 clipboard API if arboard cannot return text. The rest of the key
input remains native Slint text editing. On paste, trim the clipboard text and
replace the entire unsaved draft only after basic API-key validation; do not
log or persist the clipboard text. Existing saved keys are never disclosed.
If clipboard access is unavailable or content is unsuitable, keep the draft
unchanged and show an informative error. The explicit Save action alone writes
settings and triggers artwork refresh. No extra Flatpak permissions are added.

Reduce the editor width and height, tighten spacing, align Show and Paste
actions inside the field, and make the toggle thumb consistently white on both
ends of the track. Preserve reduced-motion behavior, focus, and controller
navigation.

## Validation

Compile and test in GNOME Builder Flatpak, including Ctrl+V, Paste, manual
typing, Show/Hide, whitespace/empty clipboard, long keys, save/cancel and
toggling with controller. The tool environment cannot verify runtime behavior.
