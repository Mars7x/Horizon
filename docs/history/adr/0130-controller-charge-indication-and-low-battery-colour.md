# ADR 0130 — Controller charging indication and critical battery colour

Status: Accepted for Phase 9.5.44.75 (pending compile and device testing)

## Context

The top-right battery indicator sometimes omitted charging even when an
attached controller was charging. SDL3 can report a valid percentage with
`PowerLevel::Unknown`; the prior presentation treated this as definitely
not charging and masked any UPower gaming-input observation. The system worker
also preferred BlueZ Battery1 over UPower, losing the latter's charging state.
The lightning-bolt path used a background fill and thin line that could blend
into its neighbouring charge fill. There was no low-battery warning colour.

## Decision

- Keep host battery, then SDL controller percentage, then single validated
  Linux peripheral fallback as the ownership/percentage priority.
- Preserve whether SDL *knows* the charging state rather than treating Unknown
  as evidence of discharge. If state is Unknown, accept a positive UPower
  charging observation only when it passes the already-existing single-pad
  ambiguity rule and differs by no more than 10 percentage points. Preserve
  known SDL charging/discharging states. No estimate or inferred charging.
- Prefer UPower Gaming Input over BlueZ for Linux fallback, because only
  UPower reports `State`. BlueZ remains the percentage-only fallback.
- Show a solid, visibly separated lightning bolt whenever the selected battery
  has a known positive charging state. No change to charging detection for
  BlueZ-only hardware that does not expose it.
- Colour the entire battery outline, fill, and lightning bolt red at <=20%;
  use separate Light/Dark red values for contrast. At >=21%, use Theme.foreground.
- Preserve the battery's existing slide/fade animations, host-first behaviour,
  source-neutral presentation, and narrow Flatpak permissions.

## Validation

Test source precedence, exact 20% threshold, SDL Unknown/known charging
conditions, invalid/ambiguous fallback, and Slint binding structure. Verify
actual SDL/UPower/BlueZ device reports, charge visibility, and contrast on
Light/Dark themes in GNOME Builder; compilation/device checks are still pending.
No additional third-party assets or dependencies.
