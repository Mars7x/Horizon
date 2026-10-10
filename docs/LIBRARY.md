# Phase 10 — Library UX

> **Current-state guide:** Library is now a **full-shell** route; it is not hosted beneath Home’s utility header. Selected-game scaling and focus spacing should match Home while leaving Home unchanged. The phase-by-phase notes below are the change history, not independent current specs; see [NAVIGATION.md](NAVIGATION.md).


## Phase 10.0: Source-backed browsing foundation

The Library route is directly accessible from the permanent **Library tile at the end of Home**, or with Menu/Start/F10 as a shortcut. The former modal route-switcher has been removed. Back returns through normal navigation history. The original Home game-card focus is unchanged.

The page uses the exact imported `LibraryGame` identities and artwork model already used by Home. Filtering (All sources / dynamically discovered providers) and four current sort modes (Alphabetical, Recently played, Time played, Recently added) are Rust-owned; the earlier Z–A mode was retired in Phase 10.2.4.4. A stable game ID preserves the selection across sorting or filtering whenever that game remains visible. The **original 10-card paginated layout was superseded by Phase 10.2**; left/right/up/down navigate the grid without entering Source/Sort controls (which are pointer-only; the top utilities remain exclusive to Home). LB changes source, RB changes sorting, and OK launches via Home's existing source-neutral launch service. Menu/Start navigates directly to Library without displaying a route switcher; Back returns through history. No Steam/Heroic metadata parsing occurs in Slint.

The first milestone is deliberately a functional browse surface, not the end of Phase 10. Search (including keyboard entry), favorites/hide, list view, duplicate resolution, and bulk library management are future Phase 10 milestones. Home recent-played ordering was added in Phase 10.1.0; the full Library grid will be visually redesigned in Phase 10.2.0.

An empty collection shows an import/install hint. An empty filter shows a distinct no-results message. Source filtering never deletes games or edits provider records. Source artwork updates and playing-state changes propagate to visible Library cards through a lightweight card-change observer.

## Validation checklist

- Start with Steam and Heroic games, select Home’s final Library tile, B returns to Home
- Move left/right/up/down across rows; held and fresh inputs traverse row edges but never wrap first-to-last
- Cycle source with LB and sorting with RB; verify pointer controls and preserved Game ID selection
- Verify page transitions retain the Library selection and Home card focus
- Check zero games, filter with zero results, 200+ installed games (bounded visible/overscan rendering after Phase 10.2)
- Launch from Library, verify source adapter/session feedback and artwork refresh
- Test controller/keyboard and pointer in GNOME/Flatpak, including Reduced Motion

## Phase 10.1.0 — Home recent-games entry

Home intentionally shows **at most 15 distinct, currently imported games with Horizon-observed play history**. The order is descending by the latest recorded play-session start. An older title with many hours does not outrank a game played yesterday. Games without observed play sessions remain in the full Library (Heroic's imported lifetime minutes are not a known last-played timestamp).

The permanent final Home tile is **Library**. It lives in the same world-space carousel track as normal games and uses the original Home focus frame, selected scale, and camera. Press Accept on the tile or click it to open Library directly. Home always shows this tile, even without activity history or installed games. The full Library catalogue and source-backed launch identifiers are never truncated to 15.

Menu/Start and F10 are direct Library shortcuts; the Phase 10.0 shell menu overlay has been removed. Back uses Navigator's route history. The existing Library browsing grid is retained temporarily; its visual redesign is Phase 10.2.0. Recent-session ordering refreshes every three seconds after persisted Activity changes, preserving the previously selected game by catalogue identity where possible. Opening Library again (for example Library → Home → Library) is a fresh visit: the first game, scrolled to the top; Source and Sort are kept.

## Phase 10.1.1 — Home initial-game fallback

The Home carousel shows up to 15 available games plus the Library tile, not only
games with recorded sessions. Verified recently played games lead, ordered by
their latest Horizon-observed session. If there are fewer than 15 such games,
the remaining places are filled alphabetically (case-insensitive A–Z) from
unplayed imported games. With no sessions, the initial Home list is alphabetical.
The Library page continues displaying **all** imported games independently of
this Home projection. Provider lifetime totals alone do not establish last
played order, and the Library tile always remains the final Home destination.

## Phase 10.1.2 — Home Library destination styling

The final Home carousel destination is visually a system tile rather than
another game cover: it retains the existing neutral square card and exact Home
focus treatment but contains only a large, centered four-square grid symbol.
The selected-title pill alone reads “Library”; there is no repeated label
inside the tile. Its click/Accept route into the full Library is unchanged.
The full Library page redesign is still reserved for Phase 10.2.0.

## Phase 10.2.0 — Artwork-first responsive Library

This phase supersedes **only the Phase 10.0 Library page layout**, not the
source-backed catalogue, sorting, launch service, or the final Home Library tile.
The Library is now a centred, artwork-only gallery: square covers are arranged
in seven columns at the reference 1280-pixel logical width, eight at 1720 pixels,
and fewer in constrained windows. Cover titles are not rendered beneath every
image; the selected game's full title and provider names appear together in a
consistent footer-like information strip beneath the gallery. The Library is a full-shell route without Home’s persistent header, top utilities, controller footer or clock.

The actual original `ui/components/focus-frame.slint` is instantiated ONCE
around the selected Library artwork. It retains the exact Home bracket paths,
Gaussian emission, accent colour cycle and reduced-motion behaviour. No
approximate menu outline or substitute focus is used for game artwork. The
existing Settings-style focus outline remains appropriate for compact Source
and Sort controls in the Library header.

The fixed **five-column/two-row, ten-cards-per-page** grid is retired. Rust
owns absolute catalogue selection and bounded virtualization, publishing only
visible and nearby rows (two overscan rows each way); Slint positions them by
absolute index. Controller/keyboard Up/Down scroll smoothly by rows. Mouse
wheel scrolls by row while keeping the focused title visible, and clicking a
cover focuses it; clicking an already-selected cover launches the game. Button
launching uses the existing Home source-neutral launch service. Sorting A–Z / Z–A
and dynamically discovered source filters remain compact and clickable.
LB/RB still cycle sorting/sources. Fresh-press edge wrapping is preserved;
a held direction stops at boundaries. The selected Game ID is preserved across
sorting and filtering when still present; opening and closing the Library also
retains its selection. Responsive column-count updates retain that identity.

Search/text entry, list view, favorites and game management remain separate
Phase 10 milestones. Do not ship nonfunctional search buttons simply for
visual symmetry. No database migrations or new Flatpak permissions are needed.

### Phase 10.2 validation

- Confirm seven artwork columns at the 1280px logical reference and eight on
  ultrawide; no fixed left-aligned empty half-screen
- Confirm only one original Home-bracket focus, gently animated, on the selected
  cover, with full untruncated selected title beneath the grid
- Scroll through 200+ items without 200 image nodes or ten-item page jumps
- Exercise controller D-pad/analogue and keyboard, including held-edge stop and
  discrete fresh-press wrap; verify source and sorting controls
- Mouse click then click again to launch, mouse wheel scroll, and go Back/Home
- Verify Steam/Heroic source-neutral launches, updates to Playing status and
  artwork, empty-state copy, responsive resize, Reduced Motion and focus memory

## Phase 10.2.2 — Full-shell Library and Home-style cover shells

The Library is a standalone full-screen route. Only Home displays the shared top utility/status bar, bottom controller footer and clock. Library uses its own title/count, compact source/sort controls, and a small Back affordance; controller/keyboard Back uses the same Rust `Navigator` history, and pointer Back calls the identical route action. No modal overlay or second header/footer is mounted over Library.

Covers are once again a rounded outer `Theme.surface-raised` **shell** with a restrained border/shadow and inset square artwork, proportionally matching Home’s `GameTile`. The original Home `FocusFrame` is reused without changing its vector paths, accent breathing, glow, timing or shell focus design. The Library controller still virtualizes the full source-backed catalogue, preserves focus by game ID, and delegates launches to Home’s established source-neutral service.

A fixed-height display window shows **only complete artwork rows**; the 180px vertical row stride, 170px horizontal card stride, shell size, clipping-safe padding, and visible-row count are defined equivalently in Rust and Slint. Column thresholds account for both outer focus gutters. The gallery is vertically centered in the usable space, leaving breathing room above and below. An extra 25px inner gutter prevents selection brackets/glow at the top or side edges from being cut off. Overscan rows remain offscreen and mount solely to permit smooth scrolling. The selected-game title, provider and position stay in Library’s own information strip, not beneath every cover or inside the removed global footer.

The next Library iteration can refine search, layout density and information design, but should not reintroduce Home chrome or replace the approved Home focus component.

## Phase 10.2.3 — Library flow polish

- Library is still a durable full-shell route. The in-page pointer Back pill has
  been removed; controller/keyboard Back and Home continue to use the existing
  Rust `Navigator` route history.
- The header starts at the standard page inset. A 29px-tall, vertically aligned
  count line gives the LINE Seed JP descenders room below the baseline.
- Source filtering and alphabetical sort remain separate controller-focusable
  actions, but share one restrained, segmented top-right control surface. Its
  source and sort values remain live and each segment is independently clickable.
  Shoulder shortcuts and existing source/sort state stay unchanged.
- The bottom-right ordinal (`1 / 36`) is no longer displayed or published.
  Only selected game title and provider remain in the stable metadata strip.
- The metadata pair animates on **selected game identity** changes, not simply
  on every publish or focus-region change. Rust emits an integer sequence *after*
  both text fields update. The Slint page caches the outgoing/new strings and
  uses a 190ms ease-out fade with a small vertical settle. Reduced Motion shows
  the new text immediately. Hidden/retained Library pages do not drive the
  metadata animation tick. This handles rapid focus steps without timers.
- Gallery camera motion follows the shared scrolling standard
  ([UI standards](UI_STANDARDS.md#scrolling)): whole-row target, never past
  the last row, `Motion.scroll-duration` linear. The unchanged `FocusFrame` continues to track the
  selected cover with `Motion.focus-duration`, including its single uniform
  accent pulse. The selected shell keeps a modest animated scale (1.045).
- The full-visible-row calculation and Rust virtualization window are preserved.
  When additional rows exist, the remaining bottom margin exposes at most 54px
  beyond the last full row, with a short bottom fade. That clipped next-row
  preview demonstrates scrolling without a permanent scrollbar. Preview card
  hit targets are disabled until their row becomes fully visible, while wheel
  scrolling remains available throughout the viewport. The hint disappears
  at the end of a filtered/sorted collection. The metadata divider remains fixed.

### Phase 10.2.3 acceptance checks

- Enter Library: no Back pill, 36 games descender intact, unified source/sort,
  no position counter, and a visible next-row peek if more rows exist.
- Move left/right and down/up using a controller or keyboard. Watch the bracket,
  cover scale, camera scroll and title/provider handoff; repeat at grid edges.
- Mouse-wheel through the peek, then click only *fully visible* artwork. Test
  both filtered and sorted lists, including an empty filter and last row.
- Check 800×450, 1280×720, 16:10 and ultrawide/fullscreen sizing, and verify
  Low Motion/Reduced Motion disables the metadata and camera animations.

## Phase 10.2.4 — Library navigation and scroll refinement

- Left/Right now follow continuous reading order across rows even during held
  input. At the beginning or end of the complete list, focus stops rather than
  wrapping to the opposite end. Up/Down remain column-preserving and bounded.
- Every visible cover owns its original Home `FocusFrame`. On a change, the
  departing cover fades its focus brackets out and the arriving cover reveals
  them in place. Brackets **never slide** horizontally or diagonally between
  positions. The cover lift/scale remains synchronized; Reduced Motion applies.
- Game title and source are compared independently when changing selection.
  An identical title or source remains fully visible and stationary. Only a
  changed text field fades/settles. Repeated Steam-to-Steam navigation does not
  reanimate the word “Steam”.
- Source and Sort are now quiet, inline header commands (no segmented raised
  capsule, no focus border and no controller/keyboard focus targets). Mouse
  clicks and the existing LB/RB source/sort shortcuts still change their values.
  Up from the first cover row never transfers focus into the header.
- The gallery viewport is stable at every scroll position. Clean-cropped
  previews of adjacent rows reveal additional games below **and** above, where
  present. The gradient fade is removed. A discreet, slender progress track
  beside the centred covers communicates current scroll position and collection
  extent, including at the final row. Previews are not clickable.
- The card window remains bounded, the Library stays full-shell, and game
  launches still delegate to the existing Home-backed source-neutral launcher.

### Phase 10.2.4 acceptance checks

- With 36 games and 7/8 columns: hold Right through row boundaries and stop at
  game 36; Left from the first game stays there. Test the same with fresh taps.
- Verify Up from row one keeps focus on the artwork. Check LB/RB and pointer
  source/sort operation with no cyan header outline or focus detour.
- Change between two Steam games: title animates, “Steam” does not; change to
  a Heroic title and the source animates once. If two game titles match, the
  title remains static while other changed metadata updates.
- Check the initial partial-next-row cue; scroll to the middle and last rows,
  where preceding cover art remains visible above the full rows. No fade is
  drawn at either boundary. The scroll position rail tracks navigation.
- Inspect Reduced Motion, empty/filtered results, pointer hit testing of
  incomplete rows, focus scale, and window sizes 800×450 through ultrawide.

## Phase 10.2.4.2 — Quiet Library chrome and shoulder controls

- Replace the upper and lower 1px gallery separator rules with a restrained,
  theme-adaptive toolbar surface and short soft shadow. No blunt grey bands,
  artwork fade, global Home chrome, or new focus destinations. The gallery
  remains independently clipped and scrollable behind these pinned areas.
- Keep Source and Sort clickable but **outside controller and keyboard focus**.
  Each uses its own compact, left-aligned two-line label/value region and a
  discrete hover surface. Fixed widths and a 20px gap avoid overlapping labels,
  especially between “All sources” and “Sort”.
- Swap the shoulder mappings: **LB = next Source**, **RB = toggle Sort**.
  Held-button repeats remain suppressed. Source identity, sorting and selection
  persistence are unchanged.
- Remove the scroll rail/thumb completely. Unfaded partial next/previous rows
  remain the only scroll-discoverability cue, with pointer input restricted to
  fully visible rows as before.
- No changes to Steam/Heroic sources, lifetime tracking, persistence, schema,
  Flatpak permissions, focus-frame geometry or animation.

### Phase 10.2.4.2 acceptance checks

- At 1280x720 and ultrawide: no horizontal separator lines or vertical rail;
  only a slight header/footer elevation, with fully clear square artwork.
- Source and Sort have visible gaps, do not overlap, and highlight subtly only
  on mouse hover. Pointer clicks perform the correct action. Up/Down never
  select the header. LB cycles Source and RB toggles Sort, including from
  the first and last gallery rows.
- Scrolling up/down shows previous/next partial rows without a gradient or an
  intrusive scroll indicator. The bottom title/provider remain stationary.
- Check light/dark/high-contrast/Reduced Motion modes.

## Phase 10.2.4.3 — Continuous Library surfaces

Phase 10.2.4.2's translucent surface rectangles and drop shadows made the
header and bottom metadata read as separate full-width dark bands. That result
was not the intended libadwaita-inspired restraint. Both tint/shadow rectangles
are removed: the fixed header content, scrolling gallery area and bottom
metadata now share the root `Theme.background`, without any visible section
line or brightness seam. The viewport remains responsible for clipping the
partial previous/next row; content never renders underneath the pinned labels.

This is only a visual-surface correction. Source/Sort hit targets and layout,
LB Source / RB Sort shortcuts, independent metadata transitions, cover focus,
scroll camera, previews and clipped pointer hit testing remain unchanged.

### Phase 10.2.4.3 acceptance checks

- Inspect the upper and lower gallery boundaries in dark, light and high-contrast
  themes. No dark tinted bands, hairline separators or shadow seams are drawn.
- After scrolling down, partial earlier covers remain visible above the full
  rows; additional rows continue to peek below without a dark gradient.
- Source and Sort remain clickable but unfocusable; LB cycles Source and RB
  toggles Sort. The title/source strip remains anchored during scrolling.
- Verify 800x450, 1280x720 and fullscreen/ultrawide configurations in GNOME
  Builder. This patch does not alter the Rust visible-row calculation.


## Phase 10.2.4.4 — Animated Source and additional sort modes

- **RB** cycles **Alphabetical → Recently played → Time played → Recently added**.
  Z–A has been removed. **LB** continues cycling the imported source filters.
- Alphabetical is case-insensitive A–Z, with Game ID as a stable tie-breaker.
- Recently played uses the latest recorded Horizon session start, not a
  provider's lifetime hours (which carry no last-played timestamp).
- Time played ranks by the largest available Steam/Heroic *source-reported
  lifetime total* for a game, or Horizon-observed time if no source lifetime
  exists. These measurements are never added. Where no playtime exists, the
  game remains visible and follows other games alphabetically.
- Recently added is the game's first-import timestamp in Horizon's local
  database; it is not the date a title was purchased or installed on Steam.
- Stable Game IDs preserve selection through filter and sort changes, with
  deterministic alphabetical ordering when metrics tie. A read-only DB query
  supplies ordering statistics; no migration or source-specific UI logic.
- The Source and Sort values animate **independently** with a restrained
  170ms fade/vertical settle. The grid has a matching slight fade-in on
  reordering. Reduced Motion uses immediate changes. Labels are pointer-only;
  nothing in the header captures controller focus.
- Ordering data refreshes on Library entry and when cycling sort modes.
  Stored values are used if a refresh temporarily fails. No UI polling is
  introduced; provider playtime remains controlled by the existing sync.

## Phase 10.2.4.5 — Larger, responsive covers and compact scroll chrome

- The Library header and selected-game metadata footer are now 99px/73px
  reservations instead of 119px/96px. The count text keeps its 30px line box
  to protect descenders, and the provider/title still have independent motion.
- Header and footer paint **exactly the same Theme.background** as the Library;
  there are no slabs or rules. Two 11px scroll-edge shadow penumbras sit just
  inside the gallery, echoing libadwaita's subtle scrolled-window elevation
  without a dark band across the whole page. No opaque overlay hides the cover
  previews.
- Cover sizes/column caps depend on logical viewport width: below 800px,
  132px/4; 800–1099px, 156px/4; 1100–1449px, 180px/5; 1450–2199px,
  206px/6 (including 1920×1080); 2200–2999px, 218px/8; and 3000px+,
  226px/10 (including 3440×1440). The actual number of columns is further
  limited by available horizontal space including both focus gutters.
- Rows use card size plus 28px; a 22px gallery gutter protects Home focus
  brackets. Visible rows and virtualization use matching Rust calculations.
  A gentle 18px upward bias within the available gallery region makes more
  useful space above/below covers without letting them collide with chrome.
- Source/Sort remain pointer-only; LB is Source, RB is Sort. Four established
  sort modes, stable game IDs, selected metadata animation, reduced motion,
  screen-edge previews, source adapters, and GameSource lifetime playtime stay
  unchanged.

Validation: compare 1280×720 window, 1920×1080, 3440×1440; resize while
selected near a row boundary, use held Left/Right and Down/Up; verify the
selected cover is completely visible, the previews aren't clickable, metadata
never overlaps artwork, and both light/dark themes show a subtle edge shadow.

### Phase 10.2.4.5 — Source/sort transition and count feedback

- Source/Sort changes now retain a **frozen outgoing overscan-window card model**
  while the new source/sort results mount. Over 230ms the previous order fades
  out as the new grid fades in, including the selected card's Home brackets.
  This replaces the earlier barely visible `0.84 → 1.0` new-grid-only fade.
- The grid position does not also slide during a source/sort transition; normal
  row scrolling keeps Home's carousel easing. Only current cards are clickable;
  outgoing snapshot cards never intercept pointer input.
- The top-left `N games` text independently crossfades and settles vertically
  over 190ms **only when the count actually changes**, retaining its 30px line
  box to protect descenders. Initial population and Reduced Motion show values
  without an unnecessary entrance animation.
- Frozen snapshots contain only previously mounted visible/overscan cards, so
  animation work stays bounded independently of the installed library size.

### Phase 10.3.1 — Source and Sort animation separation

- **Source (LB / click):** The frozen prior viewport fades away over 85ms.
  Incoming card shells then fade in and settle upward 6px, revealing by
  **column** at 7ms intervals. Each card starts at 97% shell size and eases
  to full size over 145ms, finishing in approximately 230–293ms depending
  on visible columns. The previous layout does not visibly overlap incoming
  artwork; it never receives pointer events.
- **Sort (RB / click):** The old layout fades/shrinks to 96% during 95ms,
  then the new layout eases from 96% to 100% while appearing over 140ms.
  Sort uses no stagger, keeping frequent changes quiet and predictable.
- **Both:** Preserve selected GameId across changes where available, do not
  translate the grid camera during a reorder, and retain per-tile Home focus
  brackets. Ordinary controller navigation/scrolling is unchanged. Source/Sort
  label animations and changed game-count animations remain independent.
  Reduced Motion switches both layouts immediately. These are purely visual
  changes; source queries, sorting criteria and Steam/Heroic playtime stay intact.


## Phase 10.3.2.4 — Home focus parity in the Library grid

Library selected shells now use the exact Home `Metrics.game-selected-scale`
(1.10) instead of the separate 1.045 scale. The existing `FocusFrame` keeps its
Home-authored path size and stroke thickness at its default scale factor, and
its position/bounds follow the *animated shell* with `Metrics.focus-offset` on
all sides. This applies to the live grid and its outgoing source/sort snapshot,
so the expanded and contracted focus states have consistent bracket spacing
through selection and reorder transitions. Do not modify Home's `GameTile` or
`FocusFrame`; no independent focus animation or new asset is introduced.

## Phase 10.4.3.2 — Scroll-responsive header elevation

The Library's **top** scroll-edge penumbra is no longer permanently visible. It has zero opacity at the unscrolled first row, then fades in with the actual animated gallery position during the first row of downward scrolling and fades out on return to the top. The header and page still share `Theme.background`; no permanent divider, tinted slab or artificial scroll clipping is added. The existing **bottom** boundary, responsive grid, focus brackets, selected artwork scaling, source/sort controls and Home remain unchanged. The shadow is presentation only; it never intercepts pointer/controller input. See [ADR 0161](adr/0161-scroll-driven-header-elevation-and-exclusive-game-focus.md).

## Phase 10.4.3.3 — Pointer-wheel camera separation (pending build verification)

Mouse-wheel input scrolls the Library grid viewport **without moving the keyboard/controller game selection, changing selected-title metadata, or altering the shell focus region**. Controller/keyboard directional navigation still moves selection and brings the chosen tile into view. The virtualized grid reloads the appropriate overscan window as the camera moves; Source/Sort changes restore selection-centered camera behavior. No Home geometry was modified. See [ADR 0162](adr/0162-achievement-input-and-lazy-badge-parity.md).
