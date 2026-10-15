# ADR 0094: Highest-score square grids before icons

## Status
Accepted

## Context
Horizon previously considered SteamGridDB candidate resolutions before ratings.
The user prefers square grids rather than icons and requested artwork selected
by the highest SteamGridDB score. SteamGridDB v2 returns a numeric `score` for
each artwork item, but does not provide a publisher-official flag for grids.

## Decision
- Keep strict 1:1 static-image filters and conservative game matching.
- Sort the returned eligible grid candidates by descending score; preserve
  server order for ties and rank missing scores after scored entries.
- Use the first successfully downloaded/validated candidate in score order;
  try up to 8 to handle CDN or data-validation failures.
- Fall back to scored square icons only when no eligible grid downloads.
- Keep the on/off source-art preference exactly as it is.
- Bump cache provenance schema to 2; store score with attribution and URL;
  invalidate older cached art and previous negative misses.
- Never claim a high-scoring community grid is publisher-official.

## Consequences
Horizon now favors the most highly voted compatible square grid among the
candidates returned by the API (first response page, up to 50). Vote score
selection doesn't establish authenticity. A later manual artwork picker could
allow users to select specific publisher art where desired.
