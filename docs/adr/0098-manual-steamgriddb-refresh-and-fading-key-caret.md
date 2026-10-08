# ADR 0098: Explicit SteamGridDB Refresh and Fading API-Key Caret

## Status
Accepted

## Context
Settings > Third-Party needs a user-invoked artwork refresh without deleting local cache files or navigating away. Its API-key editor has an abrupt native caret blink and lacks a smoothly animated insertion marker.

## Decision
- Add **Refresh artwork** as a controller- and pointer-accessible row following the artwork preference. Require a saved API key; keep **Remove API key** as the final row. Update the Rust Settings focus count and activation mapping consistently.
- Route this action to a dedicated SettingsController callback, then to HomeController's manual refresh path. Generation cancellation remains in force. Only manual refresh bypasses both the positive cache and recent negative-miss markers; no cache files are deleted and displayed Home art stays visible until a valid replacement arrives. Ordinary startup, key-save and preference-change flows still use cached artwork.
- Preserve source-first versus SteamGridDB-first preference rules and the highest-score square-grid-first ranking.
- Leave the Slint TextInput in charge of cursor positioning, password shaping, actual input editing, selection and native Wayland clipboard. Hide the native abrupt caret and draw a clipped 2px caret at the pixel position supplied by `cursor-position-changed`, with a smooth 1.1-second opacity cycle. Respect Reduced Motion (static insertion marker), edit mode and input-focus visibility. Existing input scrolling moves the marker with the text.
- Keep worker diagnostics internal and preserve the prior clean Third-Party page (no URL help, bottom diagnostic dump, or Back button).

## Verification
The static patch has source/ZIP checks; GNOME Builder compilation and runtime testing on native Wayland remain required. Confirm refresh downloads are performed despite a warm cache, current art does not disappear mid-refresh, gamepad focus reaches the new row, and the caret remains aligned when masking/revealing a key and scrolling a long value.
