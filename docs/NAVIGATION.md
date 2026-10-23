# Navigation

[The Rust navigator](../src/navigation/mod.rs) owns route/history/focus policy;
[presentation navigation](../src/presentation/navigation.rs) forwards resolved
state and page actions to Slint.

## Routes

- Home alone uses persistent shell chrome.
- Library is full-shell with its own grid, Source/Sort controls and focus.
- Album, Activity and Settings are three utility destinations.
- Profile is a full-shell route opened from Home's avatar, not a utility.

Back first closes temporary dialogs/editors, otherwise steps through the shared
Rust history. Routes, Settings subpages, Activity game details and Album viewers
are stable history destinations. Forward restores the page Back left, across
multiple steps; dialogs, text drafts and multi-select are not history entries.
Album sibling slides replace the current viewer address, so Back returns to the
grid and Forward reopens its last capture. New navigation clears Forward.
Global Home clears both stacks and returns to the first Home item. Menu/Start,
including a controller's + button, opens [game options](GAME_OPTIONS.md) on a
selected Home/Library game. It does not open Library; use Home's Library tile.

## Entry and focus

Library visits select the first game and scroll to the top while retaining
Source/Sort choices. Sorting within a visit keeps identity where possible.
Fresh Activity visits start at the first most-played cover; Back from its game
details restores the current cover. History rows are read-only; Up/Down scrolls
instead of focusing them. Settings sub-page Back restores its parent row;
a direct new Settings visit starts at its root, while Back/Forward restores
the recorded subpage.

Album closes selection/dialog state before history Back applies; viewer/grid
transitions participate in the same history. Profile
visits close editors and restart the friends viewport. Its avatar is a separate
Home focus slot and is included in focus snapshots. All outgoing pages reject
pointer intent. Route-local restoration is separate from the global Home reset.

## Input arbitration

Dialogs and the on-screen keyboard consume relevant input first. A page cannot
mutate another page through stale callbacks. Window activation and controller
topology changes reset held repeats; already queued old controller input must
not replay on return. See [Input](INPUT.md) and [UI standards](UI_STANDARDS.md).
