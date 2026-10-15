# Home

Home's existing visual identity is protected. Its carousel, selected title,
corner focus, selected scale, utility lift, idle motion, marquee and transitions
are references for other pages, not candidates for unrelated redesign.

## Composition

[HomePage](../ui/pages/home.slint) composes the top navigation, selected title,
carousel and footer. [HomeController](../src/presentation/home.rs) owns selection
and card projections; [catalog](../src/presentation/catalog.rs) indices are stable
for the run. Rendering components forward intent rather than choose routes.

The top row is the Profile avatar followed by Album, Activity, Achievements and
Settings. It never wraps. The avatar opens Profile and has its own focus slot.
There is no Shop, Friends or Web utility. The permanent final carousel tile opens
Library even when no games exist. Menu/Start/F10 also opens Library.

When active games exist, Home shows at most 15: recently played or newly added
games lead, with unplayed alphabetical fallback. Ordering comes from persisted
facts, never guessed lifetime totals. Changes glide while retaining a valid
selection. No production source currently populates the library, so the Library
tile remains available without active covers.

## Focus, title and media

Corner brackets track the animated selected card, not the grid slot. Utility
focus has the established ring and coupled lift. Do not recolor authored utility
SVGs or replace their scale-aware rendering. Only selected long titles marquee;
others clip with soft edges. Respect reduced motion and high contrast.

Profile pictures use Horizon's local copy or the configured Steam avatar,
including existing animation support; initials are the fallback. See
[Profile](PROFILE.md). Covers use the square/pixel-art contract in
[Artwork](ARTWORK.md).

## Clock and status

The clock uses the desktop clock-format portal preference when available,
with deterministic fallback and minute-aligned refresh. Network/battery status
comes from read-only NetworkManager, UPower and BlueZ interfaces on background
workers. Signal-driven updates and bounded rechecks prevent indefinite stale
status; transient failures are not invented readings. Native shared status glyphs
represent physical links, limited connectivity and battery/charging state.
Controller identity/battery comes from the input boundary; its artwork/glyphs
follow the last used device. These are display features, not host system controls.

Verify small windows, fullscreen, ultrawide, device changes and rapid route
transitions in the real app. See [UI standards](UI_STANDARDS.md).
