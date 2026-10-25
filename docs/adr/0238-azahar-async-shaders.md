# ADR 0238 — Azahar async shader compilation

Status: Accepted by explicit user direction. Amends ADR 0231's rule that the
bundled Azahar core is built from unmodified source.

3DS games stuttered in Horizon and not in the user's standalone Azahar.
Measured on real games, Horizon's own work per frame is a few milliseconds; the
stutter is single frames of 40–270 ms inside Azahar, most of them new Vulkan
pipelines being compiled. The user's standalone Azahar has Async shader
compilation turned on; Azahar implements it, but its libretro core never reads
the setting.

Horizon carries one patch,
[azahar-async-shader-option.patch](../../flatpak/patches/azahar-async-shader-option.patch),
applied by the Flatpak manifest to the pinned source. It adds a
`citra_async_shader_compilation` core option (default disabled, matching
upstream) and applies it with the other graphics options; Horizon answers
enabled. No other behaviour changes. The patch and the modified file carry
notices, it is distributed with the corresponding source under
GPL-2.0-or-later, and it is a candidate to send upstream. Further patches need
their own decision.
