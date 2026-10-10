# ADR 0162 — Achievement input and lazy badge parity

**Status:** Implemented. Rust and Slint compile in the Flatpak 26.08 SDK (rust-stable, aarch64) with `cargo test --all-targets --all-features`; the new tests pass. Runtime feel (held input, wheel, focus fade, badge hydration) still needs hands-on checking in the app.

## Problem

Held input repeatedly restarted a 190ms ease-in-out list transition at a 115ms cadence, making achievement history feel sticky. Pointer-wheel input was incorrectly routed through game selection in Achievements and Library. Fresh utility visits retained previous game selection and wheel position. The bounded badge prefetch displayed generic stars for achievement records with valid Steam art outside the first seven.

## Decision

- Keep game selection separate from pointer-wheel camera offsets. Wheel events are never semantic Up/Down actions, so mouse browsing cannot silently change game identity or focus. Library preserves its virtualized overscan and selection metadata while changing only viewport top.
- Use 110ms linear, Reduced-Motion-aware scroll camera steps in Achievements to avoid ease-in-out restart at every keyboard/SDL repeat. Preserve bounded history scrolling. Game-row focus reuses the Settings row reveal (fade in and out over `Motion.focus-duration`), superseding ADR 0161's immediate outline so focus motion is standard across pages.
- Route all **new entries** to Achievements through a single reset (All sources, first game, list origin). In-page Back restores the selected game. Activity refreshing the achievement preview must not trigger an Achievements navigation reset.
- Retain bounded upfront badge prefetch, then lazily hydrate all remaining badges for a game the user actually opens. Steam's schema provides image URLs; all requests are background jobs, and icon bytes remain in the existing SteamID-scoped private cache without secrets. No arbitrary network image loading in Slint. Retain an honest neutral fallback if Steam lacks an icon or the request fails. Badge cache filenames include the locked/unlocked state so an unlock never displays stale locked artwork. The importer and on-demand hydration share one download/cache helper.
- Enable read-only Activity session history wheel-scrolling without adding a focus target.
- Keep intra-Settings Back distinct from re-entry: the parent category is selected again (Appearance → Appearance, Third-Party → Third-Party, Steam Account → Steam Account row), while a new visit to Settings resets to its first row. Include a focused regression test for those paths.

## Guardrails

No Home, Friends, Steam launch, session database, Flatpak permission, dependency or migration changes. Keep provider provenance and unavailable/private behavior. Test long lists, account switching, quick hold-and-release, resize, and Reduced Motion on GNOME Builder.
