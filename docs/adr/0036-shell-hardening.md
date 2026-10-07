# ADR 0036: Phase 4 shell hardening

## Status

Accepted.

## Context

By the end of Phase 4.6 Horizon had a complete routed shell, route-local focus
memory, global Back/Home/Menu actions, retained page layers, and reduced-motion
aware transitions. The remaining Phase 4 work is resilience: device topology
changes, rapid route retargeting, transient window-size changes, and repeated
history/focus restoration must not leave stale input or presentation state.

## Decision

### Controller topology invalidates held navigation state

SDL gamepad hotplug remains entirely inside `src/input/sdl.rs`. Any controller
addition/removal invalidates analog and D-pad hold/repeat latches. This is
necessary because a disconnected device is not guaranteed to deliver the
matching button-up or centered-axis event. Resetting only when the final
controller disappeared could leave phantom navigation when another controller
remained connected.

Connected-controller status is published only when Horizon actually opens or
removes a gamepad handle.

### Route transitions have one outgoing owner

Rust publishes both the active route and the immediate transition source route.
Slint still owns interpolation, but `PageTransitionLayer` may remain visible
while inactive only when it is that immediate outgoing route. If navigation is
retargeted before the previous crossfade completes, the older outgoing layer is
hidden immediately. This prevents rapid navigation from accumulating multiple
half-faded retained pages.

The route/back-stack model itself is unchanged.

### Focus validity belongs to the focus-memory abstraction

`FocusSnapshot::normalized_for_route` and `RouteFocusMemory` enforce that a
full-shell `Utility(...)` route can only save/restore content focus. Callers no
longer duplicate that rule. Home and Library may still remember the top utility
row because they display persistent shell chrome.

### Responsive geometry tolerates transient zero-sized surfaces

The Slint root keeps the existing uniform responsive scale and expanded logical
viewport. It now guards the scale denominator against compositor-reported
zero-sized transient surfaces and clamps central shell content height to a
non-negative value. This does not change normal 16:9, 16:10, 21:9, windowed, or
fullscreen layout behavior.

## Consequences

- Disconnect/reconnect cannot inherit a stale held direction from the previous
  controller topology.
- At most one outgoing route participates in a crossfade, even under rapid
  Back/Home/route activation.
- Hidden utility rows cannot become a remembered focus target through a future
  caller mistake.
- Minimize/fullscreen/resize transitions cannot divide by a zero UI scale.
- No new source, persistence, launch, or utility-page product behavior is added.
