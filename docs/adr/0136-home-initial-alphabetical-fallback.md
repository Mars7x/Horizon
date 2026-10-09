# ADR 0136 — Fill initial Home slots alphabetically

## Status

Accepted for Phase 10.1.1; refines ADR 0135 without reversing its recent-play
priority, full-catalogue separation or permanent inline Library destination.

## Problem

Phase 10.1.0 projected only games with Horizon-observed sessions onto Home. A
freshly imported collection of 36 games displayed no covers at all, only the
Library tile. The Home launcher must be usable immediately after importing games.

## Decision

Project at most 15 imported games onto Home. Begin with the session-ranked
distinct GameIds from the Activity service, preserving their descending last
session start order. Then fill any unoccupied slots with games absent from that
observed-history list, sorted case-insensitively by full title A–Z, with stable
title/ID tie-breaks. Do not infer play timestamps from imported Heroic lifetime
hours, alphabetical order, or the act of dispatching a launcher URI.

In the no-history case, display the first 15 alphabetical games. With partial
history, recently played games precede alphabetically sorted unplayed games.
With 15 or more recent games, display only those games. With fewer than 15
installed games, display all of them. Always append the existing Library tile.

Implement this strictly in the Rust Home projection, reusing original
catalogue indices. Preserve the complete Library model and the original Home
camera, FocusFrame, animations, Playing badges, sound, and fresh-press edge wrap.
The existing three-second activity refresh continues to rebuild Home only when
its ordered game identities change, preserving selected identity where present.

## Validation

Test empty history, case-insensitive A–Z, partial history, duplicate IDs,
unknown/uninstalled IDs, 15+ played, under-15 installed, recent promotion and
eviction. Verify that the Library tile remains last and is reachable, launching
from Home uses the right catalogue identity, and Library still contains the
complete imported catalogue. Full Cargo/Slint compilation and controller
runtime tests are required before considering the patch complete.