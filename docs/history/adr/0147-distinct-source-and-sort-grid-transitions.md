# ADR 0147 — Distinct Library transitions for Source and Sort

## Context

Phase 10.2.4.5 captured a bounded outgoing grid and crossfaded it with the
incoming arrangement over 230ms. Simultaneously overlapping cover artwork
looked ghosted and failed to distinguish changing sources from sorting games.
The user approved a restrained staggered reveal for Source and softer, faster
replacement for Sort. Controller-first per-tile Home focus must remain intact.

## Decision

Keep the existing frozen outgoing overscan model and stable selection by GameId.
Publish a small source-neutral UI transition kind from the LibraryController:
0 (none), 1 (Source), 2 (Sort), before publishing the browse revision.
Slint controls the timelines using animation-tick and the existing Reduced
Motion setting, without adding asynchronous callbacks or altering persistence.

For Source, fade out the old cards over 85ms, then reveal each new card over
145ms with a 7ms stagger by grid column, 6px upward settle and shell-scale
97% to 100%. For Sort, fade/shrink the outgoing cards over 95ms and then
fade/scale in the new cards from 96% to 100% over 140ms with no stagger.
Since the phases do not overlap, artwork does not ghost between layouts.
Do not slide the focus indicator. Mouse targets belong exclusively to current
cards; incoming cards remain in the same logical positions for input.

## Consequences

Source changes take approximately 230–293ms depending on column count;
sort takes 235ms. The selected game's frame still uses the original Home
FocusFrame, while normal navigation retains its own easing. Focus, sorting,
filtering, data, migrations, permissions and dependency sets are unchanged.
Reduced Motion and initial render bypass the choreography completely.
