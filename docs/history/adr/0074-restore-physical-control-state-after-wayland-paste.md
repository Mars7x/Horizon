# ADR 0074 — Preserve held Ctrl between native Wayland pastes

## Context

The API-key editor intercepts Ctrl+V and reads the native `wl_data_device`
clipboard asynchronously. Slint 1.18.1 must receive unmodified synthetic text
key events to insert pasted characters at the live caret/selection. The prior
implementation released both Ctrl keys to permit insertion but never restored
them. If the user held Ctrl and pressed V again, Slint saw a bare `v`.

## Decision

- Track real left and right Ctrl presses and releases separately in the focused
  `TextInput`. A Ctrl+V event also provides a fallback for Ctrl held before
  the editor acquired focus.
- During synthetic paste dispatch, temporarily suppress updates to the
  physical-key tracking state, release Slint's logical Control modifiers,
  insert individual ASCII characters, and restore only the physically held
  Control keys at that moment.
- When Ctrl was physically released while the asynchronous clipboard operation
  was pending, do **not** restore it. Right Control restores as Right Control;
  no phantom left Control is left pressed.
- Repeated physical V presses while Ctrl stays held trigger distinct paste
  requests. OS-generated repeat V presses remain suppressed.
- Preserve the existing Wayland-only clipboard reader, generation/draft safety
  checks, controller navigation and native blinking caret. Do not add X11
  permissions or clipboard workarounds.

## Verification

Build with the project's Slint 1.18.1 in GNOME Builder and test: hold Ctrl,
press and release V several times while still holding Ctrl; release Ctrl before
an asynchronous paste finishes; use Right Ctrl; test a selection replacement,
show/hide and Escape. The patch was statically reviewed, but a live Slint/Wayland
runtime test is still required.
