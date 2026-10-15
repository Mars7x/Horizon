# ADR 0160 — Achievements list transition, edge treatment and keyboard repeat parity

Status: prepared in Phase 10.4.3.1, pending GNOME Builder verification.

## Context

The Phase 10.4.3 vertical browser was still visibly inconsistent with Library/Settings: games were truncated above the physical bottom edge, small shaded bands covered only part of the gallery width, source looked like a permanent button, and switching between games and details removed the outgoing view before it could animate. Large selection jumps culled rows relative to the *target* camera offset, temporarily blanking the animated viewport. Desktop keyboard auto-repeat often ran much faster than SDL controller navigation.

## Decision

- Keep a full-width header with Library's stationary, full-width 11px top-edge penumbra over the list on the same `Theme.background`; no artificial bottom viewport inset, bottom shade, footer chin or separate tinted toolbar.
- Render games as one list with uniformly bold titles and non-focusable, visually unboxed Source text, while retaining the real LB/RB/pointer source selection.
- Retain both full-width views during their Settings-family 220ms fade and 12px settle; disable pointer interactions on whichever view is inactive. Leave achievement rows unfocusable.
- Compute Slint row visibility from the **animated** game/entry camera position plus generous overscan rather than from the already-updated navigation target. Track first visible game separately, moving it only when the selection leaves the viewport in either direction. Honor Reduced Motion.
- Cap OS-generated directional keyboard repeats to the controller's D-pad delay/interval. Add key-release forwarding to reset the repeat latch, clear it when the application deactivates, and test first press, early repeats, boundary cadence, physical key release and deactivation. Non-repeatable actions remain one-shot.

## Consequences and limits

These are presentation/input changes; no provider adapter, account secret, database schema, existing utility artwork, Home UI, or source launch semantics change. Only Steam remains a verified achievement provider. The OS remains responsible for delivering key-repeat events, so very slow platform settings may under-run the controller cadence. Rust/Slint compiler verification must occur in GNOME Builder; static source checks are not compilation.
