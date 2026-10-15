# ADR 0018: Controller and Activity symbolic icons

## Decision

Use the supplied `applications-games-symbolic.svg` for the footer controller indicator and `dictionary-symbolic.svg` for the Activity utility placeholder. Preserve the original SVG files unchanged in `ui/assets`, but render their geometry as Slint `Path` components so scaling stays vector-crisp and color remains semantic/theme-driven.

Activity uses `Theme.nav-activity` (teal). Browser uses `Theme.nav-browser` (blue).
