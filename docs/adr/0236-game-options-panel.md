# ADR 0236 — Game options panel

Status: Accepted. Supersedes the floating menu and dialogs of ADR 0228. The
user found game options inconsistent: a small popup beside the cover, then
dialogs of three different sizes, a text dump for properties, a separate button
confirmation and separate error dialogs.

Game options become one surface that mirrors the in-game quick menu, at the
right edge instead of the left: a full-height panel over the dimmed page with
the quick menu's background, edge shadow, slide timing, header, compact rows and
switches, Cancel-first stacked confirmations, message line and hints. Save data,
mods, properties and confirmations are pages inside it, changed with the shared
view transition; B goes up one page and closes from the options page, + / F10
or a click beside it closes from anywhere. Failures appear on the page's
message line. Properties are captioned facts, not a text block.

Manage mods is listed only for 3DS games, the one system Horizon applies mods
to (ADR 0233). Rust still owns the page, focus, operations and stale-reply
protection; the operations themselves are unchanged.
