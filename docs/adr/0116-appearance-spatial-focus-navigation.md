# ADR 0116 — Match Appearance focus navigation to its visual rows

Status: Accepted for Phase 9.5.44.59 (pending build and runtime verification)

## Problem

In Phase 9.5.44.57 the accent picker was visually consolidated into a single
horizontal palette containing System and nine circles. The underlying Rust
navigation still treated System (focus index 3) and colours (indices 4–12) as
separate rows, so Right from System did nothing; the user had to press Down
to reach Red. Vertical motion also jumped all three Theme options to accent
System regardless of their on-screen horizontal positions.

## Decision

Keep focus and activation Rust-owned. Change only the Settings controller focus
graph; do not introduce UI-local keyboard handlers or change Slint focus IDs:

- Theme indices 0–2: horizontal wrap within System → Light → Dark.
- Accent indices 3–12: horizontal wrap within System → Red → Orange → Yellow →
  Green → Teal → Blue → Purple → Pink → White.
- UI Sounds index 13: remains a separate full-width row; Left/Right set Off/On.
- Up from an accent selects the closest Theme column. Down from a Theme option
  lands in that column and restores the last accent there, if available.
- Up from UI Sounds restores the last accent focus position; Down from an accent
  focuses UI Sounds. Bottom-to-top wrap remains available.
- The last accent focus is *transient UI state*, not a persisted appearance
  preference. Pointer activation updates it too. Merely moving focus never
  changes a saved accent or theme.

## Invariants

No Slint layout changes, new dependencies, sound assets, or Flatpak permissions.
Existing focus visuals and navigation audio still respond to semantic focus
changes, not key repeats that fail to move focus. Other Settings categories,
Third-Party API-key modal, preference persistence and Reduced Motion remain as
before.

Rust regression tests traverse the complete accent row in both directions,
verify wraparound and reversible vertical pairings, and check the UI Sounds
switch boundary. Rebuild and test with Cargo; confirm keyboard/controller focus
visually against the Appearance page before release.
