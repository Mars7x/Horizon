# ADR 0086: Perceptible Settings Focus Colour Pulse

## Status
Accepted

## Context
Phase 9.5.44.28 established a Switch-inspired outline-only Settings focus with no tinted interior or glow. The continuous 3.6-second cycle in the shared focus component remained difficult to notice, especially with saturated accent colours. The old blend factor covered only 42% of the distance from `Theme.focus` to the theme-derived `Theme.focus-highlight` (which itself is approximately 34% toward white).

## Decision
Increase the colour blend from 42% to 78% of that accent-derived highlight range while keeping the same smooth native Slint alternate-loop animation (1.8 seconds each way; 3.6 seconds total). This changes peak colour shift from about 14% to about 27% toward white. Preserve the existing focused ring's 2px border, opacity reveal, shape, and input/row styling. Do not add a halo, moving highlight, interior tint, geometric motion, or change the circular utility focus. Reduced Motion and High Contrast remain static.

## Verification
Static source validation checks the blend value, iteration/direction, and absence of fill or shadow changes. Live animation and Slint compilation still require GNOME Builder.
