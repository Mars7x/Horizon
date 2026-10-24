# ADR 0237 — 3DS save states

Status: Accepted. 3DS games had no states, which ADR 0231 left out because
Azahar's state-size query builds a complete state.

Measured on a real game, an Azahar state is about 7 MB and takes about a second
to save and a little under a second to load, at every internal resolution.
That is acceptable when the player asks for it and not otherwise. So 3DS games
get the four manual slots, quick save/load and the automatic resume point on
exit (restored at launch with Resume last state), in the shared bundle format
under `states/azahar`. They get no rewind and no timed or focus-loss autosaves,
and Azahar's size query is still never polled: the core reports state support
without asking it.

Loading takes no rollback state first, which would be a second full save:
bundles are checksummed, Azahar checks the game and build before reading a
state, and a failed load already ends the session. Saving pauses the game's
audio, and the quick menu says what it is doing. Azahar refuses states from
any other build; that follows ADR 0211.
