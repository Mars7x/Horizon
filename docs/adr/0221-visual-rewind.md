# ADR 0221 — Visual rewind

Status: Accepted. The user approved a filmstrip timeline inspired by Switch
Online and requested a distinct sound when it opens.

The existing two-core worker records volatile state/picture pairs every half
second of emulated play. The rolling window is at most 30 seconds and its
retained state/picture payload is at most 128 MiB; actual available duration is
shown, since larger DS states can exhaust the budget earlier. Native-sized
previews keep GPU upscaling from multiplying history memory. No extra core API,
dependency, disk cache, save slot or sandbox permission is introduced.

Rewind opens from the quick menu, F6 or its configurable Controller → Shortcuts
assignment (initially unset). Menu/focus-return buttons must be released before
triggering gameplay shortcuts. Earlier four-row shortcut files retain their
assignments and gain an unset fifth row, including the original single-button
format. Fast-forward's current trigger assignment is unchanged.

The game pauses while a full-shell filmstrip shows the selected preview and
relative time. Left/Right selects without wrapping; clicking a thumbnail or
dragging the scrub bar selects through the same Rust boundary. A/Enter restores that moment and resumes; B/Escape cancels and
resumes the untouched live moment. Looking never calls core restore or writes a
save. Only a successful restore discards later history. Loading a saved state
or restarting clears the history; closing the game releases it. Observed
Activity remains one session and existing durable checkpoints are retained.

States are captured/restored and DS previews composed on the game worker.
Commit waits for pending storage work, preserves the current cartridge save,
clears input/audio and resets pacing before continuing the chosen branch.
Failure stays visible and must not silently commit a different point. Window
deactivation releases held input and leaves the timeline paused. Closing while
browsing saves the real live moment, not the selected preview.

The timeline follows shared header/footer geometry, focus, typography, theme,
motion and controller glyphs. Reduced Motion removes its entrance/camera
animation. One original 160 ms descending cue plays when usable history is
published; it follows UI Sounds and the normal loudness/peak limits. Scrubbing
uses the existing quiet navigation cue; failed/repeated opens are silent.

Serialization cost, actual history duration, native/GPU DS, sustained audio and
runtime layout require core-by-core builds and runtime review on each target
architecture. An isolated Slint compile or history unit test does not establish
those properties.
