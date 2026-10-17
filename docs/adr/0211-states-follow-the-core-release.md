# ADR 0211 — States follow the core release

Status: Accepted. Supersedes ADR 0206's exact core binary hash check. The user
asked whether older save states still worked after the mGBA core was rebuilt
with its Game Boy core; they did not.

A state stays usable while the saved `core_build` names the same mGBA release
(`mGBA@<version>`) as the running core. The binary hash after `:` is still
written as provenance but no longer compared, so rebuilds of one release,
including the Game Boy build of ADR 0209, keep earlier states. The ROM identity,
metadata format, integrity hashes and preview checks are unchanged. mGBA validates
the state itself; a rejected state rolls the session back, as before. A new mGBA
release still makes states unavailable until a decision says otherwise.

State previews keep the game's native size: 240×160 for GBA and 160×144 for
Game Boy and Game Boy Color. Battery saves remain independent of states.
