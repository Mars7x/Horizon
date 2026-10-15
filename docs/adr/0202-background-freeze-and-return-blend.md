# ADR 0202 — Background freezing and continuous card return

Status: Accepted. The user requested that focus loss never open the quick menu,
an optional background freeze setting, and a transition between game and card.

Keep explicit pause-menu state separate from window activity and worker pause.
Freeze when unfocused defaults on and uses the existing launch-time core settings
snapshot. On deactivation clear keyboard/controller held input and freeze the
worker/audio without changing menu state. On activation remove only the background
freeze; a manual pause remains paused. Off permits background game/audio playback,
never background gameplay input. Core startup may produce its first frame before
freezing; restart respects the same policy. No new hosting/process API is added.

Keep the protected card press/rebound and card-origin zoom. While returning, blend
the retained game picture into the existing live card during the final part of
the shrink, before removing the player surface. Do not duplicate artwork or swap
an opaque game picture at the endpoint. Saving must still succeed before return
begins. Shared motion tokens control the blend; Reduced Motion stays immediate.

Preferences use the shared atomic store and old settings default to freezing on.
Verify actual worker freeze/continued frames, no focus-triggered menu, retained
manual pause, startup/restart deactivation, and Home/Library return rendering.
