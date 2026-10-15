# ADR 0127: GNOME symbolic network status glyphs

## Decision

Replace only the top-bar network vector drawings with user-provided GNOME
symbolic SVGs. Preserve the existing Rust source-of-truth (NetworkManager,
connectivity state/filtering, and Wi-Fi strength) and battery behaviour.
Keep each SVG byte-identical; use Slint Image colorization for dark/light mode.

Select eight relevant files: GNOME radiowaves-1/2/3/4 signal states,
radiowaves-x for disconnected, radiowaves-question for no route or unknown,
network-wired for active Ethernet, and network-wired-no-route for an Ethernet
link with limited connectivity. Do not use acquiring or wired-disconnected
icons because those conditions are not modeled distinctly in Phase 9.5.44.71.

## Visual semantics

Wi-Fi strength is 0–100 from the connected NetworkManager access point;
>=75 uses -1, >=50 uses -2, >=25 uses -3, and lower uses -4.
These number suffixes run from *strong* to *weak* in the provided icon family.
Other/unknown is not displayed as a wired connection.
All network SVG images use a common 30x28 layout cell containing 26x26 art,
without repositioning adjacent battery or utility controls.

## Provenance and licenses

GNOME radiowaves SVG metadata identifies Jakub Steiner and CC0-1.0;
Adwaita wired assets are credited GNOME Project, CC-BY-SA-3.0-US
(as permitted by Adwaita dual licensing). Unmodified source SVGs are
redistributed and colored at runtime. See THIRD_PARTY_NOTICES.md.

## Non-goals

No source-specific networking heuristics, power status changes, extra network
states, system permissions, package dependencies, or Home focus changes.
