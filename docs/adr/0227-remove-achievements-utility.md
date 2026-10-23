# ADR 0227: Remove the Achievements utility and Milestones panel

Date: 2026-10-23

## Status

Accepted. Supersedes ADR 0191’s preservation of the Achievements and Milestones
placeholders.

## Decision

At the user's request, remove the inactive Achievements page and its Home utility,
and the Milestones placeholder on Activity. Home exposes Album, Activity and
Settings, in that order. Rust owns their three-slot navigation and focus history;
the header keeps the shared icon size, spacing, focus and motion, centered as a
three-item group. Activity's read-only weekly chart spans the available width.

Remove the unused page models, callbacks, route and badge renderer. Preserve
historical achievement records/cache files, migrations and authored SVG artwork.
No achievement provider is introduced; RetroAchievements remains unimplemented.

## Verification

Route mapping, history and focus tests cover the remaining destinations and
updated icon centers. UI verification requires an actual Flatpak build and
native runtime review, including Home navigation and Activity presentation.
