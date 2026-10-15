# ADR 0085 — Outline-only Settings focus, inspired by Nintendo Switch

## Context

The Phase 9.5.44.27 Settings focus remained visually different from the Nintendo Switch Settings reference: focused rows were tinted with an accent-coloured fill and displayed a blurred drop shadow. On broad rows this appeared like a highlighted card rather than a clean selection frame.

## Decision

- Keep the current 2px rounded rectangular, dynamic-accent outline and its continuous native 3.6-second ease-in-out colour cycle.
- For **all** Settings focus sites—rows, dialog buttons, the API-key field, and the eye button—disable the shared focus surface's optional `filled` and `soft-shadow` treatments. Both settings must be explicit at each call site.
- Keep the underlying Settings row background neutral at rest, on hover, and while pressed. Do not use `accent-subtle` on row press.
- Keep secondary buttons and the eye control neutral when pressed. The primary Save action retains its **normal button fill** (`Theme.accent`), which is independent from the focus effect, so its purpose remains clear.
- Keep the existing focus entry fade and continuous, subdued accent-to-accent-highlight border animation. No rotating white gradient, extra outline, glow, or focus-driven movement.
- Preserve the utility's circular ring treatment and the separate four-corner game-title focus exactly as implemented.
- Respect Reduced Motion and High Contrast through the existing shared focus component.

## Verification

Build with Slint 1.18.1 and inspect Settings home row, Third-Party rows, API-key editor, eye icon, Cancel, Save, keyboard, and controller. Check that focus never tints the interior and that the outline alone remains legible through the entire colour cycle.
