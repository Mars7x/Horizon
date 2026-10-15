# ADR 0196 — Normalize imported titles for artwork lookup

Status: Accepted.

ROM filenames and catalog titles retain release metadata. For example,
`Pokemon - LeafGreen Version (USA)` does not exactly equal SteamGridDB's
`Pokémon LeafGreen Version`. Passing the full filename title to autocomplete
and comparing only case/whitespace prevents artwork from resolving.

Extend the shared source boundary with an optional artwork search title. The GBA
adapter removes only known trailing region/language tags and numeric revision
tags. Unknown tags and edition/hack/beta labels stay. Conflicting source title
suggestions retain the original title. Persisted titles, content hashes, catalog
recognition and ROM files are unchanged; artwork matching is not ROM recognition.

Normalize search queries and candidate comparison with Unicode canonical
decomposition, diacritic removal, case/whitespace folding and colon/dash separators.
Use the small Unicode normalization library rather than a franchise-specific
alias or incomplete handwritten accent table. No fuzzy similarity, dropped title
words or hardcoded catalog IDs are accepted. Multiple distinct matching IDs remain
ambiguous; unique authoritative external IDs retain precedence.

Advance only the disposable negative-cache marker so previous misses retry
immediately. Preserve downloaded artwork, provenance, credentials and all user
data. Network work continues through the existing background artwork service.
