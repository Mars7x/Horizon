# ADR 0138 — Centred Library Gallery with Home Game Focus (10.2.0)

## Status

Accepted as an implementation proposal; pending Rust/Slint build and runtime
validation. Supersedes Phase 10.0 Library **presentation and paging only**.

## Decision

Keep the Library route and the imported source-backed catalogue, but replace
its fixed ten-cover pages with an adaptive centred artwork-only grid. Rust
calculates column count from the logical viewport, retains an absolute selected
catalogue index, scrolls rows with overscan-window rendering, and publishes a
bounded Slint `VecModel`. A position in the model is never confused with an
absolute catalogue index. Cursor movement remains source-neutral.

Use the *existing, unchanged* Home `FocusFrame` around the selected cover,
with exactly one instance per Library view. Do not redesign or approximate the
Home bracket geometry, glow or colour pulse. Menu focus surfaces remain for the
small source and sort controls only. Place the selected title/source in a
single bottom metadata area instead of truncating a caption under each cover.

The grid is seven columns at Horizon's 1280px logical reference and eight on
large ultrawide logical canvases (three to eight within supported sizes), with
a max of two overscan rows before/after the viewport. Vertical motion uses a
200ms offset animation, disabled by Reduced Motion. Mouse-wheel movement follows
the selected item to retain a clear controller-style focus position. The
previous page counter and page jumps are removed. Sort and filter stay working,
without inventing a search experience before text-entry support exists.

## Constraints and consequences

- The full imported catalogue still belongs to Library; the 15-game Home
  projection and final Library tile are unchanged.
- All launches reuse Home's existing `launch_from_library()` service boundary;
  Playing and artwork observer updates target original catalogue indices.
- Toolbar state and selected Game ID survive reordering, source filters and
  route transitions when possible.
- A repeated directional input does not wrap at an edge; a fresh edge press can.
- No new third-party assets, dependencies, sandbox permissions or migrations.
- The Rust unit tests and full Slint build must run in GNOME Builder before
  declaring this production-complete.
