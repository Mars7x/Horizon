# ADR 0079: Visible accent-derived color cycle for focus brackets

## Status
Accepted

## Context
Phase 9.5.44.21 synchronized `Theme.focus.brighter(0.12 * cycle)` with the 3DS-inspired cursor expansion and Gaussian glow. The visual color change was too subtle to notice, particularly with saturated accents where HSV brightness multiplication can clip one or more channels at 255.

## Decision
Instead of increasing HSV brightness, interpolate the crisp bracket strokes toward the existing `Theme.focus-highlight`, which the appearance controller derives from the current accent by mixing 34% white. Use Slint's supported brush `mix` operation with the same `cursor-cycle` and `motion-strength` as expansion and Gaussian emission:

```
Theme.focus.mix(Theme.focus-highlight, 1.0 - 0.60 * motion-strength * cursor-cycle)
```

Note that Slint `mix(other, factor)` weights the *receiver* by `factor`, unlike common interpolation APIs. At peak expansion the crisp strokes are approximately 20% mixed toward white (0.60 x 0.34). At rest they are exactly `Theme.focus`. Custom and system accents remain respected, and Reduced Motion / High Contrast remain static.

Do not modify geometry, halo assets, glow opacity, cycle period, shell position, or any other animation state.

## Consequences
The actual bracket color has a more visible, subtle accent-preserving lightening/desaturation coordinated with the already-established glow and outward bracket movement. No duplicate outline is added. This effect needs visual verification in the GNOME Builder build.
