# ADR 0139 — Full-shell Library and clipping-safe cover shells

Status: Accepted for Phase 10.2.2 implementation, pending compile and visual verification.

## Context

Phase 10.2.0 retained the Home-only top status/utility row and footer while rendering Library inside the reduced Home content slot. The fixed clipped viewport exposed slivers of a third cover row; the first row's independent focus glow was also cut by the viewport boundary. Bare, edge-to-edge covers lost Home's established cover shell and visual hierarchy.

## Decision

- Only the Home route uses shared shell chrome; Library becomes a durable full-shell route like the utility destinations. Keep existing route history, Menu shortcut and back semantics. Provide a small pointer-friendly in-page Back control using the same Rust `handle_back` code path.
- The Library header retains the compact provider and sort options. The body retains the centered responsive, windowed, artwork-first gallery and selected-title/provider strip. No Home top status icons, utility row, controller legend, or clock remain in Library.
- Use Home's original round-edged surface shell, inset cover artwork and restrained shadow at Library-card scale, while instantiating the **unchanged** original `FocusFrame` once for the selected cover.
- Use a 180px vertical row stride (rather than the 170px horizontal card stride) to give the original focus glow room without exposing the first pixels of the next row. Compute only whole visible grid rows in the full viewport and reserve a 25px safe gutter around artwork for focus/glow. Center the whole-row viewport vertically. Choose responsive column counts using the full focus-safe horizontal footprint, not only the cover width, to prevent edge clipping at the 5/6-column thresholds. Render extra overscan rows offscreen, never as permanently clipped slivers.
- Keep game identity, launch/session and source filter behaviour in Rust. The Library remains a distinct screen; this is not a modal overlay or a new Home carousel.
- Register separate Rust `Weak<AppWindow>` and `Rc<NavigationController>` captures for the Library Back and viewport-change callbacks. Both callbacks are `move` closures and must own their captures independently; never reuse the values moved into a previous callback.

## Out of scope

No new search/list view, source adapters, persistent settings, Flatpak permissions or sound assets. Home visual/components/selected-bracket code is unchanged.

## Verification

Run `cargo check`, `cargo test`, and visual tests with 0, 1, 36 and 200+ games at windowed and ultrawide sizes. Check whole row visibility, first/last column focus brackets, Back via mouse/gamepad, filtering, controller-held edge handling, scroll animation and Reduced Motion.
