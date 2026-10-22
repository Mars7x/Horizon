# ADR 0224 — Shared ROM title cleanup and launch feedback

Status: Accepted. The user requested immediate launch feedback, title-only labels
across GB/GBC/GBA/DS and fixes for missing SteamGridDB artwork.

A pure domain formatter recognizes catalog region/language, numeric revision and
version, system, compatibility, enhancement and supported re-release annotations.
Display labels omit these groups; artwork queries remove known trailing groups
using the same classifier. Gold's SGB/GB compatibility and Black 2's NDSi tag
must not prevent an otherwise exact match. Meaningful editions, hack names and
unknown groups remain distinct. Catalog/database names, ROM hashes, saves and
historical identities are unchanged. This extends ADR 0196's lookup cleanup.

SteamGridDB retains its unique normalized exact-match rule and native-square
image contract. A policy-versioned miss marker retries old no-match outcomes;
successful artwork is retained. Incorrect custom filenames are not silently
renamed or fuzzily matched to another game. An explicitly identified artwork
match may use a different search name while retaining the original game/cache
identity and stored title.

The launch cue is emitted once when the in-process player accepts a request,
after successful worker creation and before the animation/first-frame wait.
It no longer waits up to 500 ms for loading. Synchronous rejection emits no cue;
a later asynchronous load failure still follows the normal visible error path.
Existing motion and audio output/warm-up policy remain unchanged.

Regression checks cover the user's Gold/Black 2 names, language/enhancement
combinations, editions/hacks, prior miss markers and unique lookup results.
The opt-in player test asserts the cue while still waiting for a first frame;
packaged audible/runtime verification remains a separate check.
