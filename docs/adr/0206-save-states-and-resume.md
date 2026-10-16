# ADR 0206: Save states and automatic resume

Status: Accepted

## Decision

Implement save states through the approved bundled mGBA libretro worker. Provide
four independent manual slots with previews, plus an automatic resume point saved
on normal game exit/application close. Resume last state defaults on under shared
Cores settings and loads the newest compatible automatic or manual state before
the first displayed frame. Turning it off affects loading, while exit still saves.
The user explicitly approved saving on exit and resuming the latest state.

The quick menu owns a quiet slot browser using shared rows, native-pixel previews,
timestamps and Cancel-first overwrite confirmation. Saving remains paused; loading
continues playback, respects background freeze and clears old input/audio. Neither
a state load nor a restart creates a new Activity session.

## Storage and failures

Each slot has a directory containing game-named state, versioned metadata and PNG
preview. Validate the full ROM identity, exact core version/binary hash, metadata
format, sizes and checksums before loading. Publish flushed complete bundles and
retain one compatible previous bundle per slot for interrupted-write recovery.
Do not prune unknown, incompatible or damaged data. Keep regular battery saves
and their backups independent; no migration or broader core API is introduced.

No states means a normal boot. If states exist but none are readable/compatible,
report the failure with guidance to disable resume, preserving existing files.
Manual errors remain in the panel. Automatic-state write failures block normal
return so the user can retry; forced termination can lose progress. There is no
periodic automatic state checkpoint, rewind, arbitrary slot count or cloud sync.

## Consequences

Core upgrades may invalidate states even when regular saves remain usable.
Preview, metadata and machine-state work runs off the UI thread. Test corruption,
backup recovery, identity isolation, preference defaults and real mGBA roundtrips,
and review the actual Flatpak slot browser with keyboard/controller actions.
