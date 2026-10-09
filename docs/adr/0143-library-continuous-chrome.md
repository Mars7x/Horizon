# ADR 0143 — Continuous Library chrome (Phase 10.2.4.3)

Status: Prepared; GNOME Builder compilation and visual validation pending.

## Problem

ADR 0142 replaced separator rules with translucent `Theme.surface` bands and
short drop shadows behind the header and bottom game metadata. In practice
those bands still formed distinct horizontal slabs with hard brightness
boundaries, contrary to the intended seamless Library design.

## Decision

Keep the existing fixed header, grid viewport, and metadata positions, but
remove both translucent rectangles and their shadow effects. Draw the Library
chrome directly on the shared `Theme.background`. This mimics the quiet, flat
state of libadwaita toolbars rather than forcing elevated toolbars at all
scroll positions. No separators, scrims, or fades are added.

The clipping for partial cover-row previews remains inside `grid-viewport`;
its boundaries, focus behavior and scrolling model are unchanged.

## Scope and invariants

One Slint component changes visually; Rust control mappings, Source/Sort
click targets, Steam lifetime playtime, launch, persistence, animations and
controller focus state are untouched. Theme colors remain token-based.

## Validation

Review the header and footer at first/middle/final scroll position on 16:9,
21:9, reduced-motion, light, dark and high-contrast themes. Compiling with
Slint 1.18.1 in the user's GNOME Builder environment remains necessary.
