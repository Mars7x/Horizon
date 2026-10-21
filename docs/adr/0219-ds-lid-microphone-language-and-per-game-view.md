# ADR 0219 — DS lid, microphone, language, imported saves and per-game view

Status: Accepted. The user asked for the remaining DS gaps.

- **Lid.** A quick-menu action closes or opens the lid and continues. melonDS
  DS only toggles it on L2 + Y, so the host presses both for one frame; the
  game also sees Y then. No other control can reach the lid.
- **Microphone.** A ninth DS control, held, sets the core's mic button with
  its built-in "blow" input. Horizon does not record from a host microphone,
  so no new permission or audio capture path is added.
- **Language.** GET_LANGUAGE answers with the system locale, so the DS
  firmware language matches it.
- **Logs.** Known, expected core notes about unoffered features are demoted to
  debug; all other warnings and errors stay visible.
- **Imported saves.** When Horizon has no save for a game, it copies once a
  same-named `.sav`, `.srm` or (DS) `.dsv` from beside the game file, inside
  the folder the user chose for games. The original is only read. This is
  the one exception to storage's rule that other applications' files aren't
  scanned.
- **Per-game view.** Quick-menu layout and swap changes are remembered in the
  game's save folder and win over the Settings default for that game. This
  replaces ADR 0217's session-only swap; writes are atomic and off the game
  thread.

RetroAchievements and cheats remain out of scope: they need a new library,
account sign-in and network use, and their own decision.
