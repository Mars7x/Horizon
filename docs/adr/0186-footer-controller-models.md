# ADR 0186: The footer's controller icon shows the controller's model

Date: 2026-10-12

## Status

Accepted. Replaces the GNOME controller icon chosen in ADR 0018.

## Context

The footer showed GNOME's generic controller whatever was connected, while
the hints already follow the controller in hand (ADR 0185).

## Decision

- **Art:** Kenney's Input Prompts 1.5A (CC0), copied byte-for-byte into
  `ui/assets/controllers/`: Xbox 360, Xbox Series (also used for Xbox One,
  which SDL doesn't tell apart), PS3, PS4, PS5, Switch Pro, left and right
  Joy-Con, Joy-Con pair, Steam Deck and a generic gamepad. Horizon-drawn
  silhouettes were tried first and rejected. GNOME's
  `applications-games-symbolic.svg` (CC-BY-SA-3.0-US) is no longer bundled.
- **Which model:** from SDL's gamepad type when a pad connects; the Deck's
  own controls (ADR 0185's detection) are the Deck. Anything else is generic.
- **Which controller:** the one used last, else the first connected,
  matching the hints. The drawing stays during the disconnect fade-out.
- **Ownership:** the model is decided in `src/input/sdl.rs` and carried in
  `ControllerStatus`; Slint only picks the image for `ControllerArt`.

## Consequences

- No CC-BY-SA artwork remains in the controller icon; the attribution check
  now requires the Kenney files.
- Steam Input's virtual pads show as Xbox, as SDL reports them.
