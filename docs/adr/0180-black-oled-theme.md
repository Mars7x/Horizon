# ADR 0180: Black theme for OLED screens

Date: 2026-10-11

## Status

Accepted.

## Context

Dark mode uses a near-black page (`#111113`) with grey surfaces. On an OLED
screen, such as a handheld, true black pixels are off: they save power and
look better in a dark room.

## Decision

Settings → Appearance → Theme gains a fourth choice, **Black**, after Dark.

- `ThemePreference::Black` resolves to `EffectiveTheme::Black`. Every
  dark-theme rule (accent contrast toward white, hover mixes, `Theme.is-dark`)
  goes through `EffectiveTheme::is_dark`, so Black behaves as Dark everywhere
  except its palette.
- Palette: page `#000000`; surfaces `#0e0e10` and raised `#18181b`, just
  lifted off black so cards, rows and the keyboard still read as shapes;
  divider `#2c2c32`. High Contrast Black is the existing High Contrast Dark
  palette, which is already true black.
- Home's top bar sits on the black page in Black, with no grey band and no
  divider line, the same way the footer has none: the icons float on black. `Theme.is-black` exists for this; pages
  otherwise use the shared tokens. *(2026-10-12: every theme now does this,
  so `Theme.is-black` was removed; Black needs no handling of its own.)*
- **System never picks Black.** The portal only reports light or dark.
- Stored as `"black"` in `config/appearance.json`; older files are unaffected.
- Appearance focus indices: Theme 0–3, accent 4–13, UI Sounds 14. Up from an
  accent goes to the nearest Theme choice by position (Pink and White sit
  under Black).

## Consequences

Pages draw from the same tokens, so no page needed its own Black handling.
Hard-coded colours chosen with `Theme.is-dark` apply to Black too.
