# ADR 0174: Remove the Web utility

**Status:** Implemented. The build and gate pass. Home was rendered with the six-icon row.

## Context

Web was a placeholder utility: a top-row icon (`gray-web.svg`) opening a page that said web access would come in a later phase. The user decided it will not be added and asked for it and every trace of it to be removed.

## Decision

- Removed `UtilityPage::Web`, `TopUtility::Web`, `AppRouteView.web`, the Web placeholder page, `WebIcon` and `ui/assets/gray-web.svg` with its REUSE entry. The top row is now **Friends, Album, Activity, Achievements, Settings, Shop**. Settings and Shop moved to indices 4 and 5, and `utility-row-width` is sized for six icons.
- The row stays centred on the screen. With an even count Activity, the default utility, sits just left of centre. Up from Home's carousel still picks the nearest icon by rendered position.
- AGENTS.md and the current feature docs now list six utilities. Older ADRs (0018, 0023, 0024, 0030, 0034, 0035, 0040) still mention Web and are left unchanged as history. This record supersedes them on that point.

## Consequences

- Nothing persisted refers to Web (focus memory is in-memory), so no migration is needed.
