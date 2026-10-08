# ADR 0123 — Inset accent swatch focus ring

## Decision

In Settings → Appearance, render each accent preset as a colour circle with a
separate, concentric outer selection/focus ring. A focused or selected circle is
36px in diameter; the ring is 48px with a 2px stroke. This leaves 4px of clear
space between the circle edge and the inner edge of the stroke. High Contrast
uses a 3px stroke and 3px of clear space. Unselected presets retain the 31px
preview size.

Use one outer ring per swatch. When focused, its colour follows the existing
shared brightness-breathing cycle without a restarting border-colour tween.
When selected but not focused, it uses the existing static selection colour.
The preview itself retains only a neutral 1px edge for light-colour contrast.

## Scope and safeguards

No changes to the other focus components, the Home game-card brackets, focus
navigation, mouse targets, persistence, accent palette, White-to-Grey theme
transition, UI sounds, dependencies, or Flatpak permissions. Reduced Motion
and High Contrast continue to use the existing shared theme/motion behaviour.

## UI Sounds subtitle

Update the Appearance UI Sounds description from navigation-only wording to
“Play sounds when navigating, confirming, and going back”. It reflects the
existing Navigation, OK and Back actions controlled by the same persisted
switch. Do not modify their WAVs, audio scheduling or action dispatch.
