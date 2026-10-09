# ADR 0145 — Responsive Library density and scroll-edge elevation

## Context

Phase 10.2.4.4 used 148px artwork with seven columns at a 1280px window and
eight at 1920px, while reserving 119px above and 96px below the gallery.
The screen looked unnecessarily empty near metadata and too dense in games.
Removing header surfaces also removed the libadwaita-style elevated-edge cue.

## Decision

Use six discrete logical-width profiles to enlarge shell/artwork together
without stretching low-resolution pixel artwork. Limit 1920px to six columns,
1280px to five, and ultrawide 3440px to ten. Use an identical layout policy
in Rust and Slint for virtualization, row counts, and focus/navigation.
Decrease chrome reservations to 99px/73px and bias the gallery upward 18px
while protecting the existing full-row/preview constraint.

Paint no independent header/footer backgrounds: instead add narrowly localized
11px Theme.shadow gradient penumbras at each content edge, with light/dark
alpha variants and no pointer target. Preserve the clean partial-row previews.

## Consequences

Fewer games appear per row, but their artwork is meaningfully larger and the
page can use its height more efficiently. Responsive resizing changes row
boundaries without changing the selected game's stable identity. The
virtualization window still has two overscan rows. No migrations, new assets,
font, source changes, dependencies, or Flatpak permissions are necessary.

## Follow-up: Grid handoff and count feedback

The 10.2.4.4 grid fade operated only on the freshly replaced model and moved
from opacity 0.84 to 1.0. It did not actually animate **between** the two
arrangements, creating an abrupt reorder with a brief flash. Phase 10.2.4.5
adds a bounded frozen outgoing model and a 230ms old/new crossfade. Reordering
also suppresses camera translation; ordinary selection scrolling still uses
Home's easing and per-tile focus semantics. Initial render and Reduced Motion
are immediate. Source changes that alter the visible game count also trigger
an independent 190ms count-label handoff. No new persistence fields are added.
