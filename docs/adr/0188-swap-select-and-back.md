# ADR 0188: Swapping select and back

Date: 2026-10-12

## Status

Accepted. Extends ADR 0185 (glyphs).

## Context

Horizon confirms with each controller's own convention: the bottom button on
Xbox, PlayStation and the Deck, the right one (A) on Nintendo pads and the
Odin 3. Some players want the other way round, such as an Xbox player used to
Nintendo's layout or the reverse.

## Decision

- **Settings → Controller**, a new category between Appearance and
  Third-Party, has two switches: **Swap Select and Back**, and **Swap
  Hints** (on by default). The labels name the jobs, not the letters, which
  differ by controller.
- It is saved in its own `config/controller.json`
  (`data_paths::controller_settings_path`, `services::controller_settings`),
  not in `appearance.json`: it isn't appearance, and later controller
  settings have somewhere to go. An unreadable file is the default (off).
- `InputManager` exchanges Accept and Back on controller actions only. The
  keyboard keeps Enter and Esc, and a clicked hint still sends its logical
  action.
- Hints keep naming logical buttons. With both switches on,
  `InputGlyphs.swap-confirm` makes `ButtonGlyph` draw a controller's logical
  A as that controller's B (○ on PlayStation) and B as its A, so the hint
  shows the button to press. Swap Hints off keeps the usual glyphs, for
  players who'd rather read the hints by job than by button.

## Consequences

- The switch applies to every controller at once, not per controller.
- Games are unaffected: the swap is Horizon's own navigation only.
- Swap Hints does nothing while the swap is off.
- A file saved before Swap Hints existed reads it as on.
