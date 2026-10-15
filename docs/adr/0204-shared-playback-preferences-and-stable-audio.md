# ADR 0204 — Shared playback preferences and stable audio

Status: Accepted following the user's request to remove the gameplay speed badge
and audio smoothing control, share focus freezing, and fix menu refresh/audio hitches.

Freeze when unfocused belongs on Settings → Cores and at the root of the existing
`cores.json`, independent of mGBA. Preserve old nested values on read, prefer an
explicit shared value, and default to on only if neither exists. Keep all other
supported core values and invalid-file/failure behavior. Reads do not rewrite
settings. mGBA retains sunlight, video filter and colour correction; the retired
audio smoothing control is ignored and the core low-pass option stays disabled.
No new core, core/process hosting interface or durable file is introduced.

Speed is a session value in the pause panel. Display no gameplay speed badge.
Pending actions guard duplicate execution without disabling every row, removing
focus styling or replacing footer hints. Preserve the shared UI components and
motion. Save/load state placeholders remain disabled.

Replace the game's contended mutex queue with preallocated atomic stereo slots.
One core worker produces, one PipeWire callback consumes; both use a compare-and-
exchange read cursor so dropping old frames on overrun cannot return overwritten
slots. No audio filtering, allocation, locks or logging runs in the callback.
Keep at most 50 ms of queued audio and build about 40 ms of headroom on startup,
resume and after starvation, with short ramps at audio boundaries. Preserve
streaming filtered 2:1 decimation and audible faster/higher-pitched 2× playback.
Pause, speed changes and restart discard queued audio and reset reader priming.
Long core/storage/device stalls can still exhaust the bounded queue.

Verify legacy false preferences, failed saves, shared/core row navigation,
stable pause-panel snapshots while pending, bounded stereo queue/reset/wrap,
concurrent overruns, brief frame jitter at both speeds and actual mGBA playback.
