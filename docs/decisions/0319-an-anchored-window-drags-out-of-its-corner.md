---
status: proposed
---

# An anchored window drags out of its corner

Supersedes: 0308 (a conversation's work opens in windows on a taskbar), the
part where an anchored window does not move.

## Problem

Agents opens anchored in the area's corner. Alex wants it beside the
conversation's middle for a while and drags its title bar, as any window's,
but it does not move: the only way out is the Keep in Corner button, which
puts it back in its old floating box, somewhere else.

## Decision

Dragging an anchored window's title bar takes it out of its corner, as
dragging a maximized one takes it out: once the press moves a few px, it
floats under the pointer at the size it had in the corner, and lands where
it is let go, floating, or docked by the column's drop boxes. Keep in Corner
shows unpressed. Pressing Keep in Corner puts it back in the corner; Escape
during the drag puts it back as it was.

Its corner size and its new floating box stay its own, so each way back puts
it where it stood.

## Alternatives

- **Keep it still**: the title bar of every other window moves it, and this
  one does nothing.
