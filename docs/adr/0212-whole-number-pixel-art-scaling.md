# ADR 0212 — Whole-number pixel-art scaling

Status: Superseded by ADR 0214, after the user found its repeated pixels visible.
It refined ADR 0201's xBRZ 6×-then-smooth-fit. The user asked for
the crispest xBRZ output on a 3440×1440 ultrawide, where the old smooth 1.5×
(GBA) or 1.67× (Game Boy) fit softened xBRZ's edges.

Smooth pixel art is shown at the largest whole-number scale that fits the player,
in physical pixels. xBRZ runs at the largest factor (2–6) dividing that scale; the
GPU repeats its pixels the rest of the way with nearest-neighbour sampling, placed
on whole screen pixels. A scale with no such factor steps down one, accepting thin
bars over resampling. The shell's design-surface scale is passed to the player so
the size is exact under it; during the card zoom the picture grows in proportion
to that size. Windows smaller than twice the game keep the 6×-and-fit path.

xBRZ cost is nearly independent of its factor, so this is a quality change, with a
smaller GPU upload as a side benefit. Screenshots and states still use native
frames. No dependency, setting or shader runtime is added.
