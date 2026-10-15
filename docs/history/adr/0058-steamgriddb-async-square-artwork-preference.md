# ADR 0058: SteamGridDB artwork download, provenance and preference

**Status:** Accepted (Phase 9.5.43)

## Decision

Keep provider-owned `ArtworkService` and its `square-v6` cache unchanged.
SteamGridDB uses its own namespaced cache and an owned data-only worker that
performs matching and image downloads; a Slint-thread poller applies finished
images to Home's `VecModel`.

- Exact Steam AppIDs through the source contract, unique-exact titles for
  Heroic/Bottles; no fuzzy or ambiguous automatic match.
- Only 512×512 or 1024×1024 static PNG/JPEG sources, bounded download and
  HTTPS SteamGridDB-host allowlist, redirects disabled, no API key on CDN.
- Reuse existing 512×512 normalization and native-square checks; preserve the
  original-source pixel-art detector, nearest-neighbour scaling, smooth Lanczos3
  and Slint `pixelated-artwork` presentation flag.
- Persist cache PNG plus author/game/grid/source URL, lookup method, pixel flag,
  checksum and age in a JSON sidecar, without overwriting launcher-managed art.
- Off: local source art first, remote only if missing. On: remote first once
  available, local source art until then/on failure; missing key disables remote.
- Each setting change increments an atomic generation; old replies are ignored.
- The source artwork image is immutable in Home state, enabling instant reset.
- Offline/network/auth/rate-limit errors preserve fallback without failing app
  startup. 30-day positive cache and 24-hour negative lookup cache reduce
  requests while allowing previously downloaded art to remain visible offline.

## Follow-up constraints

The current Settings UI cannot manually select artwork or resolve an ambiguous
name; those belong to Phase 10 Library UX. Credentials are private via Unix
permissions but not encrypted; Secret Service can be considered in Phase 12.
Flatpak's outbound network permission is not host-scoped, though each request
is restricted by the client. Include contributor credit in future art-detail UI.
