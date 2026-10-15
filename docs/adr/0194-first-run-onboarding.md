# ADR 0194 — Minimal first-run setup with shared input

Status: Accepted.

Add a first-run Welcome → Profile → Add your games → Ready flow before Home.
Keep the introduction to a welcome and one primary action. Do not add preference
toggles or provider login steps. Use system-neutral titles so more supported
formats do not require redesigning setup. GBA remains the first import backend.

Reuse Horizon's on-screen keyboard, UI sounds and transition/focus components.
Rust owns step/focus policy and prevents global route shortcuts from leaving
incomplete setup. Back dismisses the keyboard before navigating a step. Picture
choices are explicitly placeholders; preserve existing images and sources until
the user chooses a replacement. Keep ordinary desktop input functional.

Reuse the reviewed importer instead of a second onboarding-specific scan path.
The user approved mouse/keyboard selection in the system portal picker as an
exception to controller navigation within Horizon. Do not broaden sandbox grants
or inject host input to automate a portal owned by another application.

Persist confirmed progress separately from ROM import preferences. A complete
onboarding marker requires the Ready step. Save errors keep setup visible;
profile storage succeeds before Profile advances. Do not choose an emulator
API or achievements identity as part of this flow.
