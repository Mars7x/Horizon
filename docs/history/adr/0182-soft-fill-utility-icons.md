# ADR 0182: Soft-fill utility icons

Date: 2026-10-11

## Status

Accepted. Changes the artwork kept as authored by ADR 0040; that ADR's
rendering policy (SVGs drawn as they are, no recoloring in Slint) stands.

## Context

The utility icons were coloured outlines on a white fill with a thick white
sticker edge. On Dark and Black the white was the brightest thing in the
bar. Some glyphs were unclear too: Album's squiggle, Activity's open book,
and a solid grey gear that was the one icon in a different style.

## Decision

At the project owner's request the five top-utility SVGs are redrawn:

- No white edge and no white fill. Each shape is its colour's outline over
  a 20% fill of the same colour, so the bar shows through on every theme
  with a single set of files.
- The shadow is masked to fall only outside each shape, so it doesn't
  show through the see-through fill.
- Album is a photo (sun and hills), Activity a clock, Settings an outlined
  gear in a darker grey, Shop the same bag in a warmer red. The trophy
  keeps its shape; its handles and stem end inside the cup's stroke so
  nothing pokes into the see-through cup.

## Consequences

The Profile page's Friends heading drops its icon (the older white sticker
artwork, `orange-friends.svg`), leaving the plain heading.
