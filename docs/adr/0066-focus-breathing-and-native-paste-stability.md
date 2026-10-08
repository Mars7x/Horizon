# ADR 0066 — Accent breathing and reliable Wayland editor input (Phase 9.5.44.9)

## Decision

- Keep the exact four original rounded focus corner paths, stroke width, and `Theme.focus` accent. Replace the rotating-gradient treatment with a gentle 4.2-second cosine opacity breathing cycle (86–100%); freeze at 100% for Reduced Motion and High Contrast, and when the frame is inactive.
- Add controller navigation for the API-key editor: input (0), eye (1), Cancel (2), Save (3). The layout follows a 2×2 logical graph, and Save is skipped while the draft is empty. The existing controller Back always cancels. Buttons retain pointer support; eye only reveals the unsaved draft.
- Own Ctrl+V in the focused `TextInput.key-pressed` callback, before Slint's built-in clipboard shortcut runs. Remove the competing ancestor `KeyBinding`; do not allow two clipboard implementations to react to a single shortcut. Held Ctrl+V triggers only once.
- Initialize the existing standard `wl_data_device` clipboard connection when entering Settings (and retry on a keypress if unavailable). Do not request X11/XWayland or data-control permissions.
- If the clipboard worker has not yet observed its first Wayland selection, retry a failed read once after 70 ms in the worker; do not block the UI thread or retry invalid tokens.
- Snapshot the unsaved draft at paste request time. If the draft changed during the asynchronous read, ignore the returned text instead of overwriting newer typing. Existing modal-closure generation invalidation remains in place.
- Clipboard paste continues to replace the draft with the copied token, rather than inserting fragments. Empty or invalid content reports an error without changing the draft; no API key is logged.

## Validation to perform on GNOME Builder Flatpak

1. Open Settings, select Third-Party → API key, copy a token in Firefox, and press Ctrl+V only once. The draft must update once and the input must not lose focus.
2. Repeat after opening the dialog immediately, after changing the copied selection while Horizon is focused, and after selecting/caret-scrolling through a long draft.
3. Type while a paste is waiting. The late result must not replace the revised draft. Close/reopen the dialog during an outstanding read; its prior result must not appear.
4. Use gamepad D-pad input/right/down/left/up to reach eye, Save and Cancel; verify Accept and Back on every target.
5. Observe the focus outline in light/dark/accent themes, Reduced Motion and High Contrast.

Static checks are not a substitute for Rust/Slint compilation or real GNOME Wayland testing.
