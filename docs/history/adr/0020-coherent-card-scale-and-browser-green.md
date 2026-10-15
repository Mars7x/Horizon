# ADR 0020: Coherent selected-card scale and green browser utility

## Status
Accepted

## Context
The procedural fallback cover changed its own font sizes, radii, and detail
metrics while the selected shell also resized. During focus transitions this
looked like the artwork was breathing/re-layouting independently of the card.
Fallback artwork is production behavior for games with missing art, so the
artifact cannot be dismissed as test-only.

The browser utility also needs to be green rather than blue.

## Decision
- Animate selection with one `transform-scale` on the complete `GameTile` visual.
- Keep the fallback cover at stable internal typography/detail metrics; it scales
  with its parent card as one composition.
- Continue requiring high-resolution real cover sources so the modest 1.10x
  selected scale remains crisp.
- Set the browser utility through semantic `Theme.nav-browser`, colored green.

## Consequences
The placeholder and eventual real artwork share the same selection motion. The
transition is visually coherent and no artwork sub-element has its own competing
selection animation. ADR 0016's native-width/height animation requirement is
superseded by this decision.
