# ADR 0203 — Shared page history and pointer actions

Status: Accepted. The user requested multi-page mouse Back/Forward everywhere,
working Home clicks without changing drag behavior, crisp Album pixel art, and
removal of Library Add games and the Menu/+ Library shortcut.

The existing Rust Navigator owns one Back/Forward history for stable destinations:
routes, typed Settings pages, Activity game identities and Album capture paths.
Controllers publish stable page changes through the same boundary. Restoring
history must not create new entries or clear Forward. New navigation branches
clear Forward. Gallery sibling slides replace their viewer address; Back returns
to the grid. Global Home retains its explicit history reset. Temporary editors,
confirmations and multi-select are not history destinations.

A Home card click selects and dispatches the normal source-neutral launch action.
Keep the existing drag recognizer, threshold, momentum, catch and camera rules.
Home's Library tile is the Library entry point. Do not route Menu/Start/+ there,
and remove Library's Add games hint and X/Delete importer trigger. Contextual
Menu/Start behavior such as the on-screen keyboard and emulated Start is retained.
First-run folder import and durable library records remain available/intact.

Album caches use lossless PNG with a new disposable cache version. Enlarge native
still-image pixels without interpolation in all media layers; video and image
reduction keep smooth sampling. Preserve originals, metadata and media identities.
No new dependency, permission, migration, data cleanup or host package is needed.
