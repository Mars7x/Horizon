# ADR 0097: Simplify the Third-Party Settings Page

## Status
Accepted

## Context
The SteamGridDB settings view displayed a redundant personal API key documentation line, internal artwork-worker diagnostics, and a separate "Back to Settings" button below the preferences. These were unnecessary for normal configuration, especially now that artwork fetching works.

## Decision
- Keep the SteamGridDB section heading, the current Settings / Third-Party breadcrumb, and all three configuration rows.
- Remove the extra "Personal API key" help text / URL line.
- Remove the in-page SteamGridDB progress/counter telemetry. Retain the exported `artwork-status` property for existing Rust diagnostics and integration compatibility.
- Remove the redundant "Back to Settings" button. Preserve the `go-back` callback and existing controller/keyboard Back route.
- Move the preference rows upward into the unused space and retain user-facing `feedback` messages below the rows.

## Consequences
The Third-Party page becomes less cluttered without changing SteamGridDB configuration, saved keys, status collection, or normal keyboard/controller navigation. The key editor still keeps its own Cancel and Save buttons.
