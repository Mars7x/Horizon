# ADR 0132 — Balance GNOME wired status glyph visual size

Status: Proposed for Phase 9.5.44.77 (build and visual confirmation pending)

## Context

GNOME's `network-wired` and `network-wired-no-route` symbols fill their
16-unit artwork bounds more fully than the `radiowaves` Wi-Fi designs.
Displaying both groups at the same 26 × 26 logical size makes the wired
icons appear disproportionally large, especially in Horizon's header.

## Decision

- Display only the two wired variants at 22 × 22 logical pixels.
- Retain 26 × 26 logical pixels for Wi-Fi, disconnected and unknown icons.
- Keep both groups centred within the existing 30 × 28 status slot; do not
  change status cluster width, hit targets, alignment or battery placement.
- Preserve the Phase 9.5.44.76 scale-aware SVG renderer, GNOME source SVGs,
  theme tint, attribution and live NetworkManager status behaviour.
- Keep the adjustment local to the shared `NetworkSymbol` presentation.

## Verification

Build with Slint 1.18.1, compare Ethernet versus Wi-Fi visually at small
window and fullscreen scale, and verify the battery header alignment.
