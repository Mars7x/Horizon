# ADR 0118 — Menu OK/Back audio and restored focus reference

Status: accepted for the Phase 9.5.44.61 patch, pending build/visual/audio validation.

## Context

Phase 9.5.44.60 accidentally standardized other menu focus surfaces around a
fade-only border instead of the existing Settings-root accent border animation.
It also left a -2px focus lift on top utility icons, making icons look
vertically misaligned. The utility focus fill must remain removed.

## Decision

- Preserve the original Settings root accent-outline/color animation as the
  canonical single-layer menu focus; reuse its timing and the original utility
  perimeter highlight sweep across other menu outlines.
- Keep utility icon centres constant and their focus area transparent.
- Add two owner-provided, unmodified WAV cues through the existing SDL3 audio
  stream: OK for successful menu activation, Back for an actual dismissal or
  return. Do not apply these sounds to game launch or ignored presses.
- Emit menu cues from semantic Rust handlers rather than raw button/key events,
  to avoid duplicates and support controller/keyboard/pointer uniformly.
- Retain the shared UI Sounds preference, existing navigation WAV,
  fresh-press wrapping, theme/high-contrast and Reduced Motion behaviour.
- Confirm asset redistribution rights before publication.

## Scope

Presentation/audio wiring and docs only; no Flatpak permission, new dependency,
database migration, navigation mapping change or game lifecycle change.
