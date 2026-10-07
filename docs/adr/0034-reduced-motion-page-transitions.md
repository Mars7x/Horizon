# ADR 0034: Reduced-motion-aware retained route layers

- Status: Accepted
- Date: 2026-10-06

## Context

Phase 4.6 needs restrained transitions between retained shell routes without moving navigation policy into Slint. Conditional Slint route elements are instantiated only while their condition is true, so replacing one conditional page with another cannot produce a true overlapping crossfade. Horizon also already receives the host Reduced Motion preference through the appearance layer and exposes `Motion.page-duration`.

## Decision

The shell keeps one sibling `PageTransitionLayer` instance for each route. Home/Library use the central content bounds, while Friends/Album/Activity/Web/Settings use the full logical surface because utility submenu routes suppress shell chrome. Rust continues to publish exactly one active `AppRouteView`. Each layer derives only presentation properties from that value:

- active route: opacity `1`, vertical offset `0`;
- inactive route: opacity `0`, vertical offset `Metrics.page-transition-offset`;
- both opacity and vertical position animate with `Motion.page-duration` and `ease-out`;
- the active layer receives a higher stack order and owns pointer input during overlap;
- route-specific selection chrome is gated by active-route state, so retained outgoing pages cannot regain focus visuals while fading;
- top/footer chrome and the global shell menu do not animate with routes; every utility submenu hides the chrome immediately while active.

The page-transition offset is centralized at 12 px. The existing page duration remains 220 ms. When Reduced Motion is active, `Motion.page-duration` resolves to `0ms`, so the exact same bindings update immediately without a separate navigation branch.

Route components remain instantiated after fading out. This permits the outgoing and incoming surfaces to overlap and preserves transient Slint-only presentation state such as Home's carousel camera. Durable route, focus, game selection, and application data remain Rust-owned.

## Consequences

- Route changes have a short crossfade/settle rather than an abrupt content replacement.
- Reduced Motion produces an immediate page swap with identical navigation semantics.
- Pointer input cannot leak to the outgoing page while it remains visible.
- The retained shell route pages remain instantiated. This is acceptable for the fixed shell route set; production pages must keep expensive work in services/models rather than tying it to component visibility.
- Rapid route changes naturally retarget the same animated properties; Phase 4.7 will stress-test rapid navigation and back-stack behavior.
- No new third-party code or assets are introduced.
