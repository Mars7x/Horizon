# ADR 0083: Continuous Switch-Inspired Settings Focus Pulse

## Status
Accepted (pending on-device build and visual verification)

## Context
Phase 9.5.44.23 applied the compact utility focus's rotating gradient to long rectangular Settings controls, making it appear as a rapid white streak. Phase 9.5.44.25 removed the sweeping highlight, but the Settings focus only faded in once and then remained static. The selected control should retain a subtle, continuous idle animation similar to the Nintendo Switch HOME Menu.

## Decision
Extend `SelectionFocusSurface` with an opt-in `idle-pulse` property. For Settings focus indicators only, disable `rotating-highlight` and enable `idle-pulse`:

- Keep the existing 2px rounded accent border fixed in place and continuously visible.
- Once focused, smoothly interpolate the border between the exact user-selected `Theme.focus` and a softened accent-derived tint from `Theme.focus-highlight`, with no white sweep or sudden flash.
- Use a 2.6-second sinusoidal loop (continuous phase, no timers restarting on navigation). The mix factor reaches 0.52 toward `Theme.focus-highlight` at the peak, then returns to the original accent.
- Subtly modulate the existing shadow along with the border, without adding a second outline or changing geometry.
- Retain the 190ms focus acquisition/release fade in Settings.
- Under Reduced Motion and High Contrast, show a static, solid `Theme.focus` ring without pulsing.
- Preserve the original circular utility focus and its rotating gradient, including sizing and lift, as well as the separate game-title four-bracket focus.

## Consequences
The Settings rows, buttons, key field, and visibility eye control all use a consistent, controller-visible idle focus. No layout, keyboard behavior, clipboard or SteamGridDB functionality changes. All visual tuning is centralised in the existing shared focus component.
