# Navigation — current route and focus policy

Navigation decisions are Rust-owned. The original Phase 4 routing architecture has since been expanded; treat the earlier Phase 4 ADRs as historical rationale, not a limit on today's features.

## Route ownership

```text
SDL3 / keyboard / pointer intent
              |
         semantic UiAction
              |
 src/presentation/navigation.rs
              |
     src/navigation/mod.rs  -> route history / focus model
              |
    page-specific Rust controllers
              |
         ui/app.slint      -> visible retained route layers
```

`src/navigation/mod.rs` defines `AppRoute::Home`, `AppRoute::Library` and `AppRoute::Utility(UtilityPage)`; the **seven** utility pages are, in order, **Friends, Album, Activity, Achievements, Web, Settings, Shop**. `TopUtility` and Slint must agree on these positions. The Rust controller alone decides route push/pop, focus restoration and global action handling.

## Shell vs full-shell surfaces

- **Home** owns the persistent utility row and shell chrome. Its established cover, focus and marquee geometry must not be redesigned when updating another route.
- **Library** is a full-shell page with local Source/Sort controls and grid focus; it is *not* a second Home-header page.
- **All seven utilities** occupy the full-shell destination, hiding Home's chrome. Achievements has its own route and view; Friends/Album/Web/Shop are retained as their existing placeholder destinations until explicitly implemented.
- **Menu/Start opens Library**, using the same route as the Home Library tile (`NavigationController::handle_menu`). It is not currently a global modal overlay; older Phase 4 documentation describes a superseded menu design.

The current `AppRoute::uses_shell_chrome()` returns true for **Home only**. Do not copy older docs asserting that Library also displays Home chrome.

## Back, Home and route visits

- Back applies page-local Back behavior where supported (e.g. game Activity details to its overview, Achievements entries to game list, Settings subpage to parent), otherwise pops route history. At the root, Back does nothing.
- Global Home clears navigation history and brings focus to the first Home game.
- The navigation controller snapshots eligible route-local focus before changing routes and restores valid focus on Back. Full-shell routes cannot retain a hidden utility-row focus.
- Every visit to **Library** is fresh: it opens on the first game, scrolled to the top (Source and Sort are kept; they are view choices, not focus). Within a visit, re-sorting keeps the selected game.
- A **fresh** Activity visit starts on the first Most Played cover, but returning from an individual-game Activity details screen restores the same cover. Direct re-entry and route-history re-entry should agree; see Phase 10.3.3.
- Input ownership is tied to Horizon window activation; SDL topology changes clear held repeat latches. Held directional navigation should respect edge-repeat behavior rather than wrapping continuously.

## Motion and interaction

- `ui/components/page-transition-layer.slint` owns the route crossfade/settle. Motion tokens in `ui/theme/theme.slint` control speed and Reduced Motion; source/outgoing route policy remains in Rust.
- Retained outgoing layers are for visual interpolation, never alternate application-state stores. Pointer input must be gated so an inactive retained page cannot activate controls or leak a focus frame.
- Pointer, keyboard and controller activation should share the page's semantic Rust behavior. Disabled placeholders must not become focusable merely because they are drawn.
- Home focus brackets and cover scale are deliberate design references. For Library and Activity, the bracket gap must track the actual animated cover shell, not a static grid slot.
- Do not make read-only Activity session-history rows or achievement entries selectable just to support scrolling; Up/Down can scroll their viewports without focus on each row.

## References

- `src/navigation/mod.rs`, `src/presentation/navigation.rs`, `ui/app.slint`
- [INPUT.md](INPUT.md), [HOME_UI.md](HOME_UI.md), [LIBRARY.md](LIBRARY.md), [ACTIVITY.md](ACTIVITY.md), [ACHIEVEMENTS.md](ACHIEVEMENTS.md)
- Historical route/focus ADRs 0028–0036 and later Phase 9.5/10 refinements under [adr/](adr/)
