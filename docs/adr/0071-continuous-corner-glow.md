# ADR 0071 — Continuous soft glow on 3DS-inspired focus brackets

Status: experimental visual refinement (Horizon 9.5.44.14).

## Motivation

The prior bracket brightening alone did not read as an actual glow. Earlier
stacked hard-edged strokes, by contrast, appeared to be duplicate indicators.
The selected game's cover and shell must remain stationary.

## Implementation

- Preserve exactly four original solid `Path` strokes with byte-identical
  `commands`, thickness, rounded caps, corner position rules and dynamic
  `Theme.focus` colour.
- Put **one blurred alpha-mask image beneath each original Path**. Its shape
  comes directly from the same SVG path data, rendered with rounded joins
  and Gaussian-blurred (5 logical pixels). The mask contains no white-colour
  information after tinting; `Image.colorize: Theme.focus` applies the current
  theme's accent colour. Source assets are original project-generated images,
  not Nintendo artwork or another developer's visual assets.
- The mask has 18 reference units of transparent padding around each 58-unit
  source path to allow the halo to extend beyond the rounded corners.
- Glow intensity is constant while selected; it never passes sequentially
  between corners or changes into a sweep. The existing 3.8 s / 1.3 px
  bracket expansion/contraction remains, and masks move in lockstep with it.
- The original crisp path draws **after** its soft mask, preventing the glow
  from resembling a second border. No geometry, cover position, or carousel
  animation changes are made.
- Fade out the glow when focus becomes inactive, and suppress it in Reduced
  Motion and High Contrast modes. The solid accent paths remain visible.

## Assets and maintenance

`ui/assets/focus/{top-left,top-right,bottom-left,bottom-right}.png` are
first-party mask assets, generated from the existing `FocusFrame` path commands
at 5x scale using CairoSVG, then Gaussian-blurred with Pillow. Each mask is
94x94 logical units at 5x resolution (470x470 RGBA). The app does not require
CairoSVG or Pillow at runtime.

## Verification

Build in GNOME Builder (Slint 1.18.1); test dark and light themes, custom
accent colours, selection activation/deactivation, repeated controller movement,
HiDPI/ultrawide, Reduced Motion and High Contrast. Confirm that no duplicate
hard-edged focus borders appear. Rust/Slint compilation is not verified in the
artifact preparation environment.
