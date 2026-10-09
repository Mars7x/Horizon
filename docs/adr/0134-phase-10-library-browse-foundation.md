# ADR 0134 — Phase 10 Library Browsing Foundation

**Status:** Prepared for build/runtime validation.

Phase 10 begins with the previously unused Library route. A dedicated `LibraryController` owns display order, filters, selection and page slices; Slint receives only presentation rows. It consumes the same persisted library identities and `HomeController` card/artwork stream, and delegates launch to its existing source-neutral session service. This avoids a second catalogue or source-specific launch path. Visible content is limited to ten cards per page. Shell Menu now exposes Home and Library while preserving Rust-owned route history and standard focus/wrap behaviour. Game cards and their distinctive Home focus are not modified. Search, editable metadata, hiding, favorites and richer list/grid controls remain subsequent Phase 10 work.

No persistence migration, dependency, permission or third-party asset is added. Only previously available imported games are displayed. Validate Slint 1.18.1 build plus controller navigation before accepting this milestone.
