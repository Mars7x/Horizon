# Navigation

[The Rust navigator](../src/navigation/mod.rs) owns route/history/focus policy;
[presentation navigation](../src/presentation/navigation.rs) forwards resolved
state and page actions to Slint.

## Routes

- Home alone uses persistent shell chrome.
- Library is full-shell with its own grid, Source/Sort controls and focus.
- Album, Activity, Achievements and Settings are four utility destinations.
- Profile is a full-shell route opened from Home's avatar, not a utility.

Back first closes eligible page-local state, otherwise pops route history.
Global Home clears history and returns to the first Home item. Forward reopens
a page removed by route Back as a fresh visit; it does not recreate closed
page-local dialogs/details. New navigation clears the forward history.
Menu/Start/F10 opens Library through the same route as its Home tile.

## Entry and focus

Library visits select the first game and scroll to the top while retaining
Source/Sort choices. Sorting within a visit keeps identity where possible.
Fresh Activity visits start at the first most-played cover; Back from its game
details restores the current cover. History rows are read-only; Up/Down scrolls
instead of focusing them. Settings sub-page Back restores its parent row;
leaving/re-entering Settings resets to Appearance.

Album closes viewer/selection/dialog state before global Back applies. Profile
visits close editors and restart the friends viewport. Its avatar is a separate
Home focus slot and is included in focus snapshots. All outgoing pages reject
pointer intent. Route-local restoration is separate from the global Home reset.

Achievements has no provider or controller: its preserved layout displays an
unavailable state and Back only. Opening it does not trigger network/cache work.

## Input arbitration

Dialogs and the on-screen keyboard consume relevant input first. A page cannot
mutate another page through stale callbacks. Window activation and controller
topology changes reset held repeats; already queued old controller input must
not replay on return. See [Input](INPUT.md) and [UI standards](UI_STANDARDS.md).
