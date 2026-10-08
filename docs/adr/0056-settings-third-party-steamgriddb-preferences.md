# ADR 0056 — Settings / Third-Party as the SteamGridDB entry point

Status: Accepted for Phase 9.5.39

## Decision

Implement only the Settings root and its Third-Party subview; keep normal routed utility shell behavior. A Rust `SettingsController` holds selection, nesting and edit state, with semantic controller/key actions. `SettingsService` owns validation and preference commits, `ThirdPartySettings` is platform-independent, and `SettingsStore` implements a private, atomic on-disk JSON representation. No provider API/network logic belongs in Slint.

Do not fetch artwork or change Phase 9.5.38 artwork logic in this phase. Keep the toggle default Off and later implement it as a service-level priority/fallback policy, not a destructive replacement of game source assets. UI must not show the saved key or leak credentials into logs.

Use XDG config storage, not the game library database, to keep provider secrets separate from game metadata. File mode 0600 and directory mode 0700 provide same-user filesystem protection, **not encryption**. Native and Flatpak setups use their own XDG config context. Longer-term keyring/Secret Service integration requires independent Flatpak permission assessment.

## Deferred

Network key verification, SteamGridDB lookups, art downloading, contributor attribution, automatic matching, caching, on-screen keyboard, and additional Settings categories.
