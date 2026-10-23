# Activity

[Activity services](../src/services/activity.rs) and
[persistence](../src/persistence/sqlite.rs) retain observed play history and
source-reported lifetime records. No production game source currently starts new
sessions or ingests provider counters. The UI retains its totals, chart, recent
history and installed-most-played carousel.

## Measurements and recovery

Observed sessions and provider-reported lifetime are different measurements;
never add them. Missing provider data is unknown, not zero. An unavailable scan
must not overwrite an earlier valid counter. Historical tracking-method values
remain readable even when their original producer is retired.

Foreground handoff was approximate, not proof of a game's process lifetime.
Generic launch/observation services remain inactive until there is an approved
producer. Durable checkpoints protect interrupted sessions: startup closes open
sessions through their last saved checkpoint, not through arbitrary downtime.
Recovered/interrupted time must retain its limitations and state.

## Presentation

[Activity controllers](../src/presentation/activity.rs) use the shared catalog
for active covers. Retired games are excluded from installed-most-played slots,
but their rows, sessions and totals remain. A fresh visit starts at the first
cover; returning from game details restores the current cover.

The selected title may marquee; unselected long titles clip with a soft edge
and no ellipsis. Cover focus follows selected scaling. Session-history rows are
read-only and unfocusable: Up/Down scrolls the viewport, and wheel input moves
the camera without choosing rows. No invented Horizon playtime challenges.

The weekly playtime chart spans the content width. There is no Milestones panel
or Achievements utility. RetroAchievements remains unimplemented.
