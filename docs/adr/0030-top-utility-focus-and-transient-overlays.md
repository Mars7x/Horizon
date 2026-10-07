# ADR 0030: Top utility focus and transient overlays

## Status

Partially superseded by ADR 0035 in Phase 4.6.4. The focus-row decisions remain active; the transient-overlay routing decision does not.

## Context

Horizon's persistent header already displays Friends, Album, Activity, Web, and Settings, but through Phase 4.2 these icons were decorative. Controller-first navigation requires a deterministic way to move focus from page content into the header without teaching SDL/keyboard adapters about screens.

Not every header utility belongs in top-level route history. Activity and Settings are durable destinations; Friends, Album, and Web are auxiliary shell tools whose production behavior is intentionally deferred.

## Decision

- Rust owns a two-region shell focus model: page content and top utilities.
- Up from page content enters the utility row; Down returns to page content.
- Left/Right move within the five utility items and clamp at the ends. The utility strip does not wrap.
- The last selected utility is remembered when focus returns to content.
- Activity and Settings activate existing top-level routes so their durable Back/history semantics are preserved. Their route presentation owns the full shell while active: it fills the design surface and hides both top navigation and footer chrome.
- Friends, Album, and Web open transient presentation-only overlays and therefore do not enter the route back stack. Each transient overlay also owns the full shell while open and blocks pointer input from reaching the retained route underneath.
- The hidden top utility row is not a focus target while Activity or Settings is active; those routes are normalized to content focus.
- Back closes an open utility overlay before affecting route history.
- Global Home closes any utility overlay, returns focus to page content, resets to Home, and clears route history.
- Slint renders focus/overlay state and emits pointer activation callbacks; it does not decide navigation policy.

## Consequences

Header navigation is now controller/keyboard actionable without coupling input adapters to page structure. The route model still distinguishes durable Activity/Settings destinations from transient Friends/Album/Web tools, while all five utility destinations receive the same clean full-shell visual treatment. Hidden chrome is never left as an invisible focus target on Activity/Settings.

## Superseding note

ADR 0035 replaces the split route/overlay destination model. Friends, Album, Activity, Web, and Settings are now all durable `AppRoute::Utility(UtilityPage)` submenu routes with identical full-shell presentation and Back/Home semantics.
