# ADR 0010: Unify home alignment, analog navigation, and typography

## Status
Accepted

## Context
The refined home shell exposed a final set of inconsistencies before Phase 4:
footer content sat too low, the selected-game pill contained unnecessary chrome,
the carousel showed a redundant continuation arrow, top-bar groups were not
sharing one exact vertical axis, and controller navigation only recognized the
D-pad. The renderer-default typography also looked sharper/heavier than the
soft console reference.

## Decision
Treat these as shared shell/input concerns rather than one-off visual patches.

- `TopNavigation` uses one `content-center-y` for profile, navigation,
  separators, Wi-Fi, and battery geometry.
- Footer contents use the centralized `Metrics.footer-content-lift` token.
- The selected-game pill contains only centered title text.
- The carousel continuation arrow is removed; viewport movement is sufficient
  affordance for additional games.
- SDL left-stick X/Y motion is normalized into the existing directional
  `UiAction` values with enter/release hysteresis and controlled repeat timing.
- `AppWindow.default-font-family` uses the centralized `Typography.family`
  token so every present and future text element inherits one app-wide family.
  The project-wide preferred family is Inter, with slightly softer font weights. The later responsive-window/portal-clock refinement in ADR 0013 preserves this app-wide inheritance.

## Consequences
The home shell has fewer independently tuned coordinates and the input layer
continues to expose device-independent actions. Future pages get analog-stick
navigation automatically through `UiAction` rather than implementing axis
handling themselves. Typography changes can be made from one token instead of
editing each component.
