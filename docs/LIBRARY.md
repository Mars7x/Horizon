# Library

Library is a full-shell route, reached by Home's final tile.
Its established responsive square-cover grid, Source/Sort controls and scrolling
are retained. Add games and its X/Delete shortcut are absent. The
[GBA importer](ROM_IMPORT.md) remains part of first-run setup. Imported games
persist across restarts and launch through the embedded mGBA player.
Empty-state labels reserve a full-height line box so glyph descenders remain
visible at different UI scales, including the empty-filter message.

## Ownership

[LibraryController](../src/presentation/library.rs) projects the shared
[append-only catalog](../src/presentation/catalog.rs) into the
[Library page](../ui/pages/library.slint). Indices remain stable for the process;
reloads append and mark unavailable games rather than moving old slots.

Source filtering never edits provider records. Sort modes are Alphabetical,
Recently played, Time played and Recently added. Source-reported lifetime and
observed playtime stay distinct; provider totals do not establish last-played
timestamps. The generic reload service remains but has no current user trigger.

## Interaction contract

A fresh visit selects the first item at the top while keeping filter/sort
choices. Re-sorting within a visit preserves game identity when possible.
D-pad/keyboard movement follows the responsive rows, does not wrap first-to-last,
and keeps the animated selected cover in view. LB/RB change filter/sort where
available; pointer controls use the same Rust-owned selection rules. Wheel input
pans without changing the focused game.

Selected scale, corner brackets and title clipping/marquee match Home. Top
header elevation follows camera offset; it is absent at zero. Empty filters have
a distinct no-results message. Artwork changes propagate through the shared card
observer. Retired identities are not active games; they remain only in
[historical persistence](DATABASE.md).

Display names omit known catalog region, language, revision/version, system and
hardware-enhancement annotations through the shared ROM title formatter. Stored
catalog names, ROM identities and meaningful editions/hack names remain intact.
