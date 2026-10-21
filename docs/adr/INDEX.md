# Current decisions

| ADR | Decision |
| --- | --- |
| [0190](0190-flatpak-embedded-emulator-direction.md) | Desktop Flatpak with a future embedded-emulator direction |
| [0191](0191-steam-profile-and-friends-only.md) | Steam profile pictures/friends only; independent SteamGridDB |
| [0192](0192-repository-maintenance.md) | Current contracts, isolated history and repository checks |
| [0193](0193-reviewed-gba-import.md) | Local GBA recognition and reviewed content identity before core integration |
| [0194](0194-first-run-onboarding.md) | Minimal welcome, local profile setup and shared controller input |
| [0195](0195-automatic-folder-import.md) | Direct folder selection with automatic scanning and import |
| [0196](0196-rom-artwork-title-matching.md) | Source-aware release tags and normalized unique artwork title matching |
| [0197](0197-offline-hash-title-recognition.md) | Offline ROM-hash catalog naming for new and existing imports |
| [0198](0198-first-gba-playback.md) | In-process mGBA libretro worker for the first embedded GBA player |
| [0199](0199-curated-core-preferences.md) | Curated per-core settings and shared controller A/B swap |
| [0200](0200-quiet-pause-panel.md) | Quiet pause panel with session speed, safe restart and native screenshots |
| [0201](0201-gba-colour-and-pixel-art.md) | GBA colour correction and pixel-art smoothing |
| [0202](0202-background-freeze-and-return-blend.md) | Independent background freezing and continuous return to the game card |
| [0203](0203-shared-page-history-and-pointer-actions.md) | Shared page history, working pointer activation and crisp Album images |
| [0204](0204-shared-playback-preferences-and-stable-audio.md) | Shared playback freezing, stable quick-menu feedback and bounded game audio |
| [0205](0205-human-readable-storage.md) | Purpose-first storage, readable game folders and reserved state slots |
| [0206](0206-save-states-and-resume.md) | Manual state slots and automatic save-on-exit/resume |
| [0207](0207-jxl-only-album-pictures.md) | Canonical JXL raster storage, animation and retained durable-file compatibility |
| [0208](0208-controller-remapping-and-playback-speed.md) | GBA controller remapping and four session playback speeds |
| [0209](0209-game-boy-and-game-boy-color.md) | Game Boy and Game Boy Color through the bundled mGBA core |
| [0210](0210-live-game-folder.md) | The chosen game folder is checked again so new games appear |
| [0211](0211-states-follow-the-core-release.md) | Save states follow the mGBA release, not its exact binary |
| [0212](0212-whole-number-pixel-art-scaling.md) | Smooth pixel art scales by whole numbers (superseded by 0214) |
| [0213](0213-game-boy-display-options.md) | Game Boy palette, Super Game Boy border and GBC colour correction |
| [0214](0214-screen-size-xbrz-and-display-paced-frames.md) | xBRZ resized to the screen, filtering on its own thread, frames paced to the display |
| [0215](0215-deleted-games-leave-the-library.md) | Games deleted from the folder are retired, and return if their file does |
| [0216](0216-nintendo-ds-playback.md) | Nintendo DS through the bundled melonDS DS core, in-process |
| [0217](0217-ds-screen-layouts-and-mapping.md) | DS screen layouts, swapping and button mapping |
| [0218](0218-ds-gpu-upscaling-and-dev-speed.md) | DS GPU upscaling, faster launches, crash-safe resume and representative dev builds |
| [0219](0219-ds-lid-microphone-language-and-per-game-view.md) | DS lid, microphone, language, imported saves and per-game view |
| [0220](0220-in-game-shortcuts.md) | In-game shortcuts: fast-forward, quick save/load, screenshot |
| [0221](0221-visual-rewind.md) | Bounded volatile rewind with a filmstrip timeline and original opening cue |
| [0222](0222-playback-controls-and-ds-resolution.md) | Two-speed toggle, paired face-button swap and DS scaling through 8× |
| [0223](0223-image-only-album.md) | Album retains screenshots and animated images; clip playback removed |

Earlier decisions and their supporting phase notes are in
[history](../history/INDEX.md). Their numbered identifiers remain reserved.
Read the relevant current feature guide and actual source before using historical
rationale. Accepted direction is not proof of implemented future functionality.
