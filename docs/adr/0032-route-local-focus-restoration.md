# ADR 0032: Route-local shell focus restoration

## Status

Accepted — Phase 4.4.

## Context

Phase 4.3 introduced Rust-owned focus transfer between Home content and the
persistent top utility row. The focus object was global, however, so opening a
different top-level route could carry the source route's active utility focus
into the destination route. Back also restored only the route, not the focus
state the user had left on that route.

This becomes increasingly fragile as Library, Activity, and Settings gain their
own focusable content.

## Decision

Horizon stores one durable `FocusSnapshot` per `AppRoute`.

A snapshot contains:

- the shell focus region (`Content` or `TopUtilities`);
- the last selected `TopUtility` for that route.

The snapshot deliberately excludes temporary reciprocal Home game↔utility
anchors. Those anchors are valid only for an immediate spatial reversal within
one Home visit and must never cross a route boundary.

Before a route change, `NavigationController` saves the source route's snapshot.
After the route changes, it restores the destination route's snapshot.

First visits start in content focus. Activity's default utility identity is
Activity and Settings' is Settings. Back restores the previous route's saved
focus. Global Home clears route history and explicitly returns Home to content
focus. Transient utility overlays do not alter route focus memory.

## Consequences

- Route changes no longer leak header focus from one page to another.
- Back restores both the previous route and its prior shell-focus context.
- Future page-local focus regions can extend the same restoration model without
  moving policy into Slint or SDL.
- Home's geometry-aware reciprocal pairing remains ephemeral and independent of
  top-level navigation history.
