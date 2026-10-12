# ADR 0185: Button glyphs follow the keyboard or controller in use

Date: 2026-10-12

## Status

Accepted.

## Context

Every hint and header glyph was drawn as an Xbox button (A, B, X, LB, RB),
whatever the user held. On a keyboard, a PlayStation pad or a Nintendo pad
the hints named buttons that don't exist there, and the keyboard had no
bumper keys at all.

## Decision

- **Families.** SDL's gamepad type sorts a controller into Xbox,
  PlayStation, Nintendo or Steam Deck; unknown types are Xbox. The Deck is
  recognised by Valve's vendor and Deck product IDs, or by any Valve
  controller on a machine whose DMI board is `Jupiter`/`Galileo` (Steam's
  virtual gamepad in Game Mode reports Valve's IDs but not the Deck's).
- **Nintendo convention.** On Nintendo pads the right button (A) confirms,
  the bottom (B) goes back and the top (X) is the secondary action, so the
  printed letters mean the same as on other pads.
- **Keyboard bumpers.** Q and E are LB and RB. A focused text field takes
  printable keys first, so typing is unaffected.
- **Last used wins.** The glyphs are those of the keyboard or controller
  used last; mouse and touch keep the current ones. Startup shows a
  connected controller's, else the keyboard's.
- **Look.** Monochrome, in the existing glyph style, with each family's own
  symbols: Enter/Esc/Del/Q/E key caps; ✕ ○ □ △ drawn as shapes; L1/R1
  (PlayStation, Deck), L/R (Nintendo), LB/RB (Xbox).
- **Ownership.** Detection and the style live in `src/input/`
  (`ControllerFamily`, `GlyphStyle`, `InputManager`); Slint only reads the
  `InputGlyphs.family` global. Hints stay logical button names, so clicking
  one still sends the same action.

## Consequences

- Hints match the device in hand, including after switching mid-session.
- Home's footer hint widens for the keyboard's Enter cap.
- Third-party pads SDL can't type show Xbox glyphs.
- Nintendo pad users' B/A swap compared with earlier builds.
