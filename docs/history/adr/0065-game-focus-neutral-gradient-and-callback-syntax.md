# ADR 0065 — Game-focus rotating gradient and Slint callback declarations (9.5.44.8)

## Context

The existing Home game focus mark consists of four rounded, separated corner
paths. Its older 4-second accent highlight sweep was too conspicuous. The user
requested a gradual rotating brightness distribution without changing the focus
indicator's silhouette or geometry. The 9.5.44.7 Wayland paste patch also failed
Slint compilation because two callbacks declared their handlers inline using
`callback name(...) => { ... }`, which Slint 1.18.1 does not permit.

## Decision

- Keep the four existing `Path` commands, positions, `Metrics.focus-arm`,
  `Metrics.focus-stroke`, line caps, and stroke join completely unchanged.
- Replace only the brush: one broad, soft, 7.2-second rotating linear gradient
  from 80–100% opacity on the same color. Use white in dark mode and the existing
  system focus color in light mode for legibility.
- Disable rotation under Reduced Motion; leave a fully solid static focus mark.
  High-contrast retains a solid `Theme.focus` brush. Only the game focus frame is
  changed, not utility focus, cover motion, or carousel transitions.
- Separate the callback declaration and handler for `settings-apply-wayland-paste`
  (`ui/app.slint`) and `apply-wayland-paste` (`ui/pages/settings.slint`). Both
  follow the Slint 1.18 callback declaration `callback foo(string);` with a
  separate `foo(value) => { ... }` handler.

## Verification

The patch should be built in GNOME Builder and checked in light/dark mode,
Reduced Motion, and with a selected game. Also test native Wayland Ctrl+V;
Phase 9.5.44.8 corrects callback syntax, but does not by itself prove that the
Wayland clipboard integration runs successfully. No new dependencies or assets.
