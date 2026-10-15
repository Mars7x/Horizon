# ADR 0199 — Curated core preferences and controller mapping

Status: Accepted following the user's request for Settings → Cores → core name →
important settings, and for Swap Select and Back to affect cores.

Keep the existing bundled mGBA host. Add no new cores, plugin/process APIs,
downloads or BIOS management. Settings lists only installed mGBA and exposes
a manual Sunlight level (0–10, default zero), useful for solar-sensor games.
[ADR 0204](0204-shared-playback-preferences-and-stable-audio.md) removes the former
audio smoothing control and makes background freezing a shared preference. No hardware
sensor interface is granted. The user's subsequent filter approval adds a Video
filter choice: None, LCD and Smooth; [ADR 0201](0201-gba-colour-and-pixel-art.md)
adds colour correction and Smooth pixel art. None remains the default; LCD is an original
3× software subpixel/grid treatment on the worker, and Smooth uses Slint's native
bilinear scaling. This introduces no RetroArch shader-file compatibility or new
shader runtime/assets. Advanced performance/debug controls remain internal.

Preferences apply at the next launch and are resolved on the game worker before
core loading. A serial settings worker loads/saves `cores.json` through the shared
atomic settings store. Publish only committed values; preserve invalid files and
previous preferences on failures. Rust owns nesting, selection and Back memory;
Slint uses the existing Settings rows, breadcrumb and reduced-motion transitions.

Controller gameplay A/B follow Swap Select and Back after device-family mapping.
Keyboard Z/X remain unchanged. Swap Hints changes only hints. Other gameplay
buttons, held state, menu repeats and pause behavior retain their contracts.
This updates the initial mapping policy in ADR 0198 without changing hosting.

Verification includes held A/B combinations, preference round trips and failures,
real pinned-core audio filtering and option queries, and actual Settings runtime
navigation/save/error checks. Physical controller and architecture verification
remain separate from semantic tests.
