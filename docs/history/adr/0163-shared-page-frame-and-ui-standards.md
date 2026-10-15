# ADR 0163 — Shared page frame and UI standards

**Status:** Implemented. Compiles; `cargo test`, clippy and fmt pass in the Flatpak 26.08 SDK. Pages were checked with offscreen renders (light and dark); hands-on controller testing is still pending.

## Problem

Each full-screen page drew its own header, rows, text overflow and back affordance. Titles sat at four different heights; subtitles came and went; Settings used a divider instead. Long text was cut with "…" on some pages and marqueed on others. Row heights were 81–94px. Sub-pages offered "‹ Activity", "‹ Games", or a non-clickable "Settings / Third-Party". Library, Achievements and Activity built the same "caption over value" header control three ways. No page told controller users which buttons did what.

## Decision

The user chose: title-only headers, marquee-on-focus with no ellipsis anywhere, controller hints on every full-screen page, and the shared layout for placeholder pages (visual only; no Friends features). On that basis:

- Shared components in `ui/components/`: `PageHeader`, `HeaderValue`, `Breadcrumb`, `ScrollEdgeShadow` (`page-header.slint`), `ListRow` and `Surface` (`list-row.slint`), `MarqueeText`, and `HintBar`. Pages compose these instead of re-implementing them.
- New `Metrics` page-frame tokens, a `label-size` type token, and `value-swap-duration` / `list-camera-duration` motion tokens.
- The header title is the section name and stays fixed across sub-views; sub-pages name themselves in a clickable breadcrumb.
- Hints are computed in one Slint mapping (`page-hints` in `app.slint`) from state Rust already publishes, so pointer actions and background updates refresh them without extra plumbing. What a button *does* stays in Rust; the mapping only labels it and must be kept in step.
- Library's Rust layout constants (`CONTENT_TOP`, `CONTENT_BOTTOM`) follow the new header and bottom-bar heights.
- Supersedes ADR 0160/0161 statements that the Achievements list runs to the physical bottom edge with no bottom shadow: lists now end at the bottom bar and show a bottom shadow while more rows remain.

## Guardrails

Home is unchanged (verified pixel-identical in renders). No route, history, focus or input behaviour moved into Slint. Read-only rows remain unfocusable.
