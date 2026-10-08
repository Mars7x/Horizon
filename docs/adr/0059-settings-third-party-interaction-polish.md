# ADR 0059 — Third-Party Settings interaction polish (Phase 9.5.44)

## Decision

Keep the existing SettingsController as the single owner of route, selection,
validation and persisted SteamGridDB preferences. Replace the temporary
presentation with a consistent set of Horizon-styled rows and buttons, including
an animated switch for the artwork priority preference.

The API key editor uses Slint's built-in TextInput primitive inside an app-styled,
clipped field. It supports desktop clipboard paste (Ctrl+V and an explicit Paste
button), select-all (Ctrl+A), masked entry with optional temporary reveal, and
horizontal cursor following for long keys. Opening the editor schedules focus
handoff for the next UI tick to avoid routing key events through the shell's
controller/keyboard focus scope; closing it restores shell focus via existing
AppWindow behavior. The stored secret never populates the editor. Feedback is
cleared on fresh editing sessions and cancellation.

Keep one semantic selection target per Settings row. For the preference, controller
Accept or pointer click toggles while Left explicitly selects false and Right
selects true. When no key is stored, skip the disabled removal action during
navigation. Avoid unnecessary preference persistence and artwork reload on
no-op Left/Right repeat.

## Motion and accessibility

Use small focus-border, hover, page fade/settle, switch-track/knob and modal
fade/settle animations. Honor `Motion.reduced-motion` across every added
animation. No row or dialog relies on scaling to signal selection.

Rows and buttons expose accessibility roles and labels; the preference exposes
switch checked state, and the input is a password-mode native text primitive.

## Scope

Presentation and keyboard focus polish only. The API, artwork caches,
SteamGridDB attribution, storage permissions and artwork priority semantics
from Phases 9.5.40–9.5.43 are unchanged. Do not embed third-party media.
