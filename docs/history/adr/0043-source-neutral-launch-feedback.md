# ADR 0043: Source-neutral launch feedback

## Status

Accepted for Phase 7.0.5.

## Context

XDG OpenURI launch dispatch can succeed before Steam or another source brings the game to the foreground. Without immediate presentation feedback, pressing Accept can look like a no-op and encourages repeated launch requests.

Launch feedback must not be implemented as Steam-specific UI or inferred from source process lifetime. Horizon already has a generic `GameLaunchService` and an active-window handoff boundary.

## Decision

`HomeController` owns transient source-neutral launch presentation state. A fresh Accept on the selected game immediately publishes a pending launch index and `Launching…` status before dispatching through `GameLaunchService`. While pending, repeated Accept and carousel Left/Right input are ignored.

The selected card uses the existing real-dimension rendering path to perform a restrained press-in transition; no raster subtree transform is introduced. The selected-title pill keeps its fixed geometry and shows the game title plus status. Reduced Motion makes the press transition instantaneous while preserving the textual state.

A synchronous launch failure changes the status to `Launch failed`. When Horizon loses OS window activation, the pending feedback is cleared because foreground ownership has been handed away. Controller ownership continues to follow ADR 0041 independently.

## Consequences

- Users receive immediate visible acknowledgement after pressing a game.
- Repeated Accept cannot dispatch duplicate launches while a handoff is pending.
- Steam, Heroic, Lutris, and later providers can use identical launch feedback.
- Launch feedback does not claim that a process is running; it only represents Horizon's launch/handoff state.
