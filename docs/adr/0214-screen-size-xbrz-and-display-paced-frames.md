# ADR 0214 — Screen-size xBRZ and display-paced frames

Status: Accepted. Supersedes ADR 0212. In fullscreen on a 3440×1440 175 Hz display
the user saw visible pixels with whole-number xBRZ (each xBRZ pixel repeated 3×3)
and no smoother motion at 2×–4×.

Smooth pixel art now runs xBRZ at the fitted scale rounded up (at most 6×) and
resizes the result on the CPU to the screen's exact size by area averaging, shown
1:1. Edges blend across at most one screen pixel; flat areas are unchanged; the
picture fills the height with no bars. xBRZ and the resize are split into bands
across up to four threads; the banded xBRZ is tested byte-identical to one pass.

The core worker no longer filters. It hands each new frame to a filter thread
that filters the newest frame only when the player has taken the last one. The
player takes at most one frame per display refresh, read from the monitor through
winit (60 Hz if unknown, refreshed each second). Slint's rendering notifier was
tried and rejected: it is never called under Skia on Vulkan. Fast-forward therefore
shows up to the display's refresh rate without slowing the game.

Measured on a Ryzen 7 5800X3D: xBRZ plus resize 4.4 ms a frame at 3440×1440 and
8 ms at 4K; 4× with Smooth pixel art shows about 175 frames a second on a 175 Hz
display. Screenshots and states still use native frames. No dependency, setting
or permission is added.
