# ADR 0030: Top utility focus and transient overlays

## Status

Accepted for Phase 4.3.

## Context

Horizon's persistent header already displays Friends, Album, Activity, Web, and Settings, but through Phase 4.2 these icons were decorative. Controller-first navigation requires a deterministic way to move focus from page content into the header without teaching SDL/keyboard adapters about screens.

Not every header utility belongs in top-level route history. Activity and Settings are durable destinations; Friends, Album, and Web are auxiliary shell tools whose production behavior is intentionally deferred.

## Decision

- Rust owns a two-region shell focus model: page content and top utilities.
- Up from page content enters the utility row; Down returns to page content.
- Left/Right move within the five utility items and clamp at the ends. The utility strip does not wrap.
- The last selected utility is remembered when focus returns to content.
- Activity and Settings activate existing top-level routes.
- Friends, Album, and Web open transient presentation-only overlays and therefore do not enter the route back stack.
- Back closes an open utility overlay before affecting route history.
- Global Home closes any utility overlay, returns focus to page content, resets to Home, and clears route history.
- Slint renders focus/overlay state and emits pointer activation callbacks; it does not decide navigation policy.

## Consequences

Header navigation is now controller/keyboard actionable without coupling input adapters to page structure. Transient utility surfaces can later be replaced with production implementations without polluting the top-level route model or changing input semantics.
