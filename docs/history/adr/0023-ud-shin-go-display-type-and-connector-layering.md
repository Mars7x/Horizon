# ADR 0023: UD Shin Go display typography and connector layering

## Status
Accepted.

## Decision

Horizon keeps Inter as the general UI typeface, but requests UD Shin Go NT for the clock and selected-game title. Clock digits use Bold weight, the AM/PM suffix uses DemiBold, and selected-game titles use DemiBold/Semibold.

UD Shin Go NT is not redistributed by Horizon. It is a commercial Morisawa font and must be supplied by the user's environment under an appropriate license.

The carousel shelf is rendered before the title connector. The connector and endpoint dot are then drawn above it using one `Theme.divider` color. This prevents the translucent shelf from tinting the connector at the intersection.

## Consequences

- Missing UD Shin Go NT falls back through the platform font system.
- Do not bundle Morisawa font files unless redistribution rights are separately established.
- The connector color must remain visually continuous across the shelf boundary.
